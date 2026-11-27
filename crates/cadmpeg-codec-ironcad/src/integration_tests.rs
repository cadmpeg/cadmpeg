// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, InspectOptions};
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions, DecodeResult};

use cadmpeg_ir::unknown::part::{gives_up, record_unreadable_kind};

use crate::loss::IronCadLossCode;
use crate::test_support::corpus;
use crate::IronCadCodec;

const BOX: &str = "Box_50x40x25.ics";
const CYLINDER: &str = "Cylinder_50x50.ics";
const SCENE: &str = "Ironcad_2025_simple_test_parts.ics";
const ACIS_BLOCK: &str = "Block_ACIS_100x50x20.ics";
/// Agreement of a decoded corner with the modelled block dimensions.
const EPS_POINT_MM: f64 = 1.0e-9;
/// Agreement of a stored rotation coefficient with the cosine or sine of the
/// modelled angle.
const EPS_ROTATION: f64 = 1.0e-12;
/// Agreement of a converted density with the stored steel density.
const EPS_DENSITY_KG_PER_MM3: f64 = 1.0e-15;

fn detect(bytes: &[u8]) -> Confidence {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let confidence = IronCadCodec.detect(&ctx, root).unwrap();
    ctx.finish_session().unwrap();
    confidence
}

fn decode(name: &str) -> DecodeResult {
    IronCadCodec
        .decode(&mut Cursor::new(corpus(name)), &DecodeOptions::default())
        .unwrap()
}

fn given_up(result: &DecodeResult) -> usize {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| gives_up(&loss.code))
        .count()
}

fn count(result: &DecodeResult, code: IronCadLossCode) -> usize {
    let code = code.note("").code;
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == code)
        .count()
}

fn layer_formats(result: &DecodeResult) -> Vec<String> {
    result
        .report()
        .dialects()
        .unwrap()
        .iter()
        .map(|layer| layer.format().to_owned())
        .collect()
}

#[test]
fn scenes_are_detected() {
    for name in [BOX, CYLINDER, SCENE] {
        assert_eq!(detect(&corpus(name)), Confidence::High, "{name}");
    }
}

#[test]
fn compound_file_without_scene_identity_is_refused() {
    let bytes = cadmpeg_test_support::compound::compound_fixture();
    assert_eq!(detect(&bytes), Confidence::No);
    assert!(IronCadCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .is_err());
    assert!(IronCadCodec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .is_err());
}

#[test]
fn inspect_lists_scene_streams() {
    let summary = IronCadCodec
        .inspect(&mut Cursor::new(corpus(BOX)), &InspectOptions::default())
        .unwrap();
    let names: Vec<_> = summary
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    for stream in ["Brep Data", "Shape Data", "Document Data", "Preview", "HSF"] {
        assert!(names.contains(&stream), "{stream} in {names:?}");
    }
}

#[test]
fn single_part_scenes_hold_two_parasolid_records() {
    for name in [BOX, CYLINDER] {
        let result = decode(name);
        assert_eq!(
            count(&result, IronCadLossCode::FeatureShapeRetained),
            1,
            "{name}"
        );
        assert_eq!(given_up(&result), 0, "{name}");
        assert_eq!(
            count(&result, IronCadLossCode::ParasolidBodyPartial),
            0,
            "{name}"
        );
        assert_eq!(result.ir().geometry.bodies.len(), 1, "{name}");
        assert_eq!(count(&result, IronCadLossCode::KernelPrologueUnreadable), 0);
        assert_eq!(count(&result, IronCadLossCode::RecordHeaderUnrecognized), 0);
        let formats = layer_formats(&result);
        assert_eq!(formats[0], "ironcad");
        assert_eq!(
            formats
                .iter()
                .filter(|format| *format == "parasolid")
                .count(),
            2,
            "{name}"
        );
    }
}

#[test]
fn mixed_kernel_scene_holds_parasolid_and_acis_records() {
    let result = decode(SCENE);
    assert_eq!(count(&result, IronCadLossCode::FeatureShapeRetained), 7);
    assert_eq!(given_up(&result), 0);
    let mut faces: Vec<_> = result
        .ir()
        .geometry
        .bodies
        .iter()
        .map(|body| {
            result
                .ir()
                .geometry
                .faces
                .iter()
                .filter(|face| face.id.as_str().starts_with(body_scope(body.id.as_str())))
                .count()
        })
        .collect();
    faces.sort_unstable();
    assert_eq!(faces, [3, 3, 6, 6, 10]);
    assert_eq!(count(&result, IronCadLossCode::KernelPrologueUnreadable), 0);
    assert_eq!(count(&result, IronCadLossCode::KernelUnrecognized), 0);
    let formats = layer_formats(&result);
    assert_eq!(
        formats
            .iter()
            .filter(|format| *format == "parasolid")
            .count(),
        10
    );
    assert_eq!(formats.iter().filter(|format| *format == "acis").count(), 2);
}

#[test]
fn parasolid_layers_carry_the_schema_token() {
    let result = decode(BOX);
    let layers = result.report().dialects().unwrap();
    for layer in layers.iter().filter(|layer| layer.format() == "parasolid") {
        let schema = layer
            .declared()
            .iter()
            .find(|(key, _)| key.as_str() == cadmpeg_parasolid::DECLARED_SCHEMA)
            .map(|(_, value)| value.as_str());
        assert_eq!(schema, Some("SCH_3401176_34101_13006"));
    }
}

#[test]
fn undecoded_scene_streams_are_named() {
    let result = decode(BOX);
    let pending: Vec<_> = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == IronCadLossCode::SceneStreamsPending.note("").code)
        .map(|loss| loss.message.as_str())
        .collect();
    assert!(pending
        .iter()
        .any(|message| message.contains("`Shape Data`")));
    assert!(!pending
        .iter()
        .any(|message| message.contains("`Brep Data`")));
    assert!(!pending.iter().any(|message| message.contains("`HSF_LIB`")));
}

#[test]
fn container_depth_reads_no_records() {
    let options = DecodeOptions {
        depth: cadmpeg_core::decode::DecodeDepth::Container,
        ..DecodeOptions::default()
    };
    let result = IronCadCodec
        .decode(&mut Cursor::new(corpus(SCENE)), &options)
        .unwrap();
    assert!(result.ir().geometry.bodies.is_empty());
    assert_eq!(count(&result, IronCadLossCode::FeatureShapeRetained), 0);
}

#[test]
fn acis_block_decodes_one_closed_box_in_the_part_frame() {
    let result = decode(ACIS_BLOCK);
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| !gives_up(&loss.code)));
    assert_eq!(count(&result, IronCadLossCode::FeatureShapeRetained), 1);
    assert_eq!(count(&result, IronCadLossCode::BodyExtensionsRetained), 1);
    let geometry = &result.ir().geometry;
    assert_eq!(geometry.bodies.len(), 1);
    assert_eq!(geometry.faces.len(), 6);
    assert_eq!(geometry.edges.len(), 12);
    assert_point_box(&result, [-50.0, -25.0, 0.0, 50.0, 25.0, 20.0]);
}

#[test]
fn parasolid_box_decodes_one_closed_box_in_the_part_frame() {
    let result = decode(BOX);
    let geometry = &result.ir().geometry;
    assert_eq!(geometry.bodies.len(), 1);
    assert_eq!(geometry.faces.len(), 6);
    assert_eq!(geometry.edges.len(), 12);
    assert_point_box(&result, [-25.0, -20.0, 0.0, 25.0, 20.0, 25.0]);
}

/// The identity prefix that the entities of a qualified body share.
fn body_scope(body: &str) -> &str {
    body.find("/brep:").map_or(body, |end| &body[..=end])
}

/// Asserts the low and high corners of the box of every decoded point.
fn assert_point_box(result: &DecodeResult, expected: [f64; 6]) {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for point in &result.ir().geometry.points {
        let position = point.position().get();
        for (axis, value) in [position.x, position.y, position.z].into_iter().enumerate() {
            low[axis] = low[axis].min(value);
            high[axis] = high[axis].max(value);
        }
    }
    for (value, expected) in low.iter().chain(&high).zip(expected) {
        assert!((value - expected).abs() <= EPS_POINT_MM, "{low:?} {high:?}");
    }
}

/// `BOX` with every frame magic of its streams cleared, so that the frame of
/// `Brep Data` does not read.
fn box_without_frame_magic() -> Vec<u8> {
    let mut bytes = corpus(BOX);
    let magic = 0x3de3_de00_u32.to_le_bytes();
    let mut cleared = 0;
    for at in 0..bytes.len().saturating_sub(magic.len()) {
        if bytes[at..at + magic.len()] == magic {
            bytes[at..at + magic.len()].fill(0);
            cleared += 1;
        }
    }
    assert!(cleared > 0);
    bytes
}

#[test]
fn a_brep_data_frame_that_does_not_read_is_one_unreadable_give_up() {
    let bytes = box_without_frame_magic();
    let result = IronCadCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .unwrap();
    assert_eq!(count(&result, IronCadLossCode::FrameUnreadable), 0);
    let given_up: Vec<_> = result
        .report()
        .losses
        .iter()
        .filter(|loss| gives_up(&loss.code))
        .collect();
    let [loss] = given_up.as_slice() else {
        panic!("one give-up loss: {given_up:?}");
    };
    assert_eq!(loss.code, record_unreadable_kind());
    assert!(loss.message.contains("Brep Data"), "{}", loss.message);
    assert_eq!(result.ir().native_unknowns("ironcad").unwrap().len(), 1);

    let mut options = DecodeOptions::default();
    options.strictness.pinned = true;
    IronCadCodec
        .decode(&mut Cursor::new(bytes), &options)
        .expect_err("a pinned decode refuses an unreadable frame");
}

/// `BOX` with the start sector of its `Document Data` directory entry moved
/// outside the allocation table, so that the stream does not open.
fn box_without_document_data_chain() -> Vec<u8> {
    let mut bytes = corpus(BOX);
    let name = "Document Data"
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let name_length = u16::try_from(name.len()).unwrap().to_le_bytes();
    let entries = (0..bytes.len().saturating_sub(128))
        .filter(|&at| {
            bytes[at..at + name.len()] == name[..] && bytes[at + 64..at + 66] == name_length
        })
        .collect::<Vec<_>>();
    let [entry] = entries.as_slice() else {
        panic!("one `Document Data` directory entry: {entries:?}");
    };
    bytes[entry + 116..entry + 120].copy_from_slice(&0x00ff_fff0_u32.to_le_bytes());
    bytes
}

#[test]
fn a_document_data_stream_that_does_not_open_is_one_unreadable_give_up() {
    let intact = decode(BOX);
    assert!(intact
        .ir()
        .model
        .product_definitions
        .iter()
        .any(|definition| definition.is_root() && definition.units.is_some()));

    let result = IronCadCodec
        .decode(
            &mut Cursor::new(box_without_document_data_chain()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let given_up: Vec<_> = result
        .report()
        .losses
        .iter()
        .filter(|loss| gives_up(&loss.code))
        .collect();
    let [loss] = given_up.as_slice() else {
        panic!("one give-up loss: {given_up:?}");
    };
    assert_eq!(loss.code, record_unreadable_kind());
    assert!(loss.message.contains("Document Data"), "{}", loss.message);
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    let root = result
        .ir()
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .unwrap();
    assert!(root.units.is_none());
}

#[test]
fn parasolid_records_rewrite_into_framed_neutral_transmits() {
    for name in [BOX, CYLINDER, SCENE, "Block_Parasolid_100x50x20.ics"] {
        let bytes = corpus(name);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let snapshot = crate::container::open(&ctx, root).unwrap();
        let entry = snapshot.stream(crate::container::BREP_DATA).unwrap();
        let stream = snapshot.open(&ctx, entry).unwrap();
        let payload = crate::frame::payload(&ctx, stream).unwrap();
        let records = crate::tree::read(payload).unwrap().records;
        for record in records
            .iter()
            .filter(|record| record.kernel == crate::tree::Kernel::Parasolid)
        {
            let neutral = cadmpeg_parasolid::native::to_neutral(record.bytes.window()).unwrap();
            let framed = cadmpeg_parasolid::records::PrimaryRecords::read(&neutral)
                .unwrap()
                .unwrap();
            assert!(
                framed.is_terminated(),
                "{name}: stopped at {:?}",
                framed.stop_offset()
            );
            assert!(!framed.records().is_empty(), "{name}");
        }
        drop(snapshot);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn scene_parts_are_placed_by_their_public_access_matrices() {
    let result = decode(SCENE);
    assert_eq!(count(&result, IronCadLossCode::PlacementBoundByOrder), 1);
    assert_eq!(count(&result, IronCadLossCode::PlacementUnbound), 0);
    assert_eq!(count(&result, IronCadLossCode::AssemblyBoundByOrder), 1);
    assert_eq!(count(&result, IronCadLossCode::AssemblyFlattened), 0);
    let model = &result.ir().model;
    let mut names: Vec<_> = model
        .occurrences
        .iter()
        .map(|occurrence| occurrence.name.as_deref().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "Assembly",
            "Box_Parasolid",
            "Compound_part",
            "Cylinder_ACIS",
            "Part4",
            "Part6"
        ]
    );
    let translation = |name: &str| {
        let occurrence = model
            .occurrences
            .iter()
            .find(|occurrence| occurrence.name.as_deref() == Some(name))
            .unwrap();
        let rows = occurrence.transform.unwrap().affine_rows();
        for (axis, row) in rows.iter().enumerate() {
            for (column, value) in row[..3].iter().enumerate() {
                let expected = if axis == column { 1.0 } else { 0.0 };
                assert!((value - expected).abs() <= EPS_POINT_MM, "{name}: {rows:?}");
            }
        }
        rows.map(|row| row[3])
    };
    assert_eq!(translation("Box_Parasolid"), [0.0; 3]);
    let block = translation("Part6");
    let cylinder = translation("Part4");
    assert!((block[2] - (cylinder[2] + 50.0)).abs() <= EPS_POINT_MM);
    let root = model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .unwrap();
    assert_eq!(root.occurrences.len(), 4);
    assert!(root.bodies.is_empty());
}

#[test]
fn single_part_scenes_place_their_part_at_the_origin() {
    let result = decode(BOX);
    let occurrence = &result.ir().model.occurrences[0];
    assert_eq!(
        occurrence.transform.unwrap().affine_rows(),
        cadmpeg_ir::transform::Transform::identity().affine_rows()
    );
    assert_eq!(count(&result, IronCadLossCode::AssemblyFlattened), 0);
}

#[test]
fn part_properties_mass_and_scene_units_are_transferred() {
    let result = decode(SCENE);
    let model = &result.ir().model;
    let part = model
        .product_definitions
        .iter()
        .find(|definition| definition.source_name.as_deref() == Some("Compound_part"))
        .unwrap();
    assert_eq!(
        part.native_ref.as_deref(),
        Some("{D511A408-89F1-4138-8BAE-FC2721E6DBAE}")
    );
    let height = part
        .bom_properties
        .iter()
        .find(|(key, _)| key.as_str() == "SizeboxHeight")
        .map(|(_, value)| value.parse::<f64>().unwrap())
        .unwrap();
    assert!((height - 0.075).abs() <= EPS_POINT_MM);
    assert!(!part
        .bom_properties
        .keys()
        .any(|key| key.as_str() == "PartMass" || key.as_str() == "MassDensity"));
    assert_eq!(model.mass_properties.len(), 6);
    let density = model.mass_properties[0].density.unwrap().get();
    assert!((density - 7.8e-6).abs() <= EPS_DENSITY_KG_PER_MM3);
    assert!(model.mass_properties[0].mass.is_none());
    let root = model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .unwrap();
    let units = root.units.as_ref().unwrap();
    assert_eq!(units.length, cadmpeg_ir::design::units::UnitSymbol::Mm);
    assert_eq!(units.angle, cadmpeg_ir::design::units::UnitSymbol::Deg);
    assert_eq!(units.mass, Some(cadmpeg_ir::design::units::UnitSymbol::Kg));
}

#[test]
fn every_face_carries_its_kernel_colour() {
    let result = decode(SCENE);
    let faces = &result.ir().geometry.faces;
    assert!(faces.iter().all(|face| face.color.is_some()));
    let beige = cadmpeg_ir::topology::Color::new(230.0 / 255.0, 211.0 / 255.0, 198.0 / 255.0, 1.0);
    let parasolid_faces = faces
        .iter()
        .filter(|face| face.id.as_str().contains("/brep:face#"))
        .count();
    assert_eq!(
        faces.iter().filter(|face| face.color == beige).count(),
        parasolid_faces
    );
}

/// The affine rows of the occurrence named `name`.
fn placement(result: &DecodeResult, name: &str) -> [[f64; 4]; 3] {
    result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some(name))
        .unwrap()
        .transform
        .unwrap()
        .affine_rows()
}

/// Asserts the rotation block and the translation of `rows`.
fn assert_placement(rows: [[f64; 4]; 3], rotation: [[f64; 3]; 3], translation: [f64; 3]) {
    for (row, expected) in rows.iter().zip(rotation) {
        for (value, expected) in row[..3].iter().zip(expected) {
            assert!((value - expected).abs() <= EPS_ROTATION, "{rows:?}");
        }
    }
    for (row, expected) in rows.iter().zip(translation) {
        assert!((row[3] - expected).abs() <= EPS_POINT_MM, "{rows:?}");
    }
}

const IDENTITY: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

#[test]
fn a_moved_block_is_placed_at_its_translation() {
    let result = decode("Block_moved.ics");
    assert_placement(
        placement(&result, "Block_Parasolid_100x50x20"),
        IDENTITY,
        [30.0, -20.0, 10.0],
    );
    assert_point_box(&result, [-50.0, -25.0, 0.0, 50.0, 25.0, 20.0]);
}

#[test]
fn a_rotated_block_is_turned_30_degrees_about_z() {
    let result = decode("Block_rotated.ics");
    let (sin, cos) = 30.0_f64.to_radians().sin_cos();
    assert_placement(
        placement(&result, "Block_Parasolid_100x50x20"),
        [[cos, -sin, 0.0], [sin, cos, 0.0], [0.0, 0.0, 1.0]],
        [0.0; 3],
    );
}

#[test]
fn an_assembly_matrix_places_its_member_parts() {
    let result = decode("Assembly_moved.ics");
    assert_eq!(count(&result, IronCadLossCode::AssemblyBoundByOrder), 1);
    assert_eq!(count(&result, IronCadLossCode::AssemblyFlattened), 0);
    assert_placement(
        placement(&result, "Assembly1"),
        IDENTITY,
        [30.0, -20.0, 10.0],
    );
    assert_placement(placement(&result, "Part1"), IDENTITY, [0.0; 3]);
    assert_placement(placement(&result, "Part2"), IDENTITY, [0.0, 0.0, 25.0]);
    let model = &result.ir().model;
    let definition = |name: &str| {
        model
            .product_definitions
            .iter()
            .find(|definition| definition.source_name.as_deref() == Some(name))
            .unwrap()
    };
    let members: Vec<_> = definition("Assembly1")
        .occurrences
        .iter()
        .map(|id| {
            let occurrence = model.occurrences.iter().find(|o| &o.id == id).unwrap();
            occurrence.name.as_deref().unwrap()
        })
        .collect();
    assert_eq!(members, ["Part1", "Part2"]);
    let root = model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .unwrap();
    assert_eq!(root.occurrences.len(), 1);
}

/// The names of the occurrences that the definition `name` holds.
fn members(result: &DecodeResult, name: Option<&str>) -> Vec<String> {
    let model = &result.ir().model;
    let definition = model
        .product_definitions
        .iter()
        .find(|definition| match name {
            Some(name) => definition.source_name.as_deref() == Some(name),
            None => definition.is_root(),
        })
        .unwrap();
    definition
        .occurrences
        .iter()
        .map(|id| {
            let occurrence = model.occurrences.iter().find(|o| &o.id == id).unwrap();
            occurrence.name.clone().unwrap()
        })
        .collect()
}

#[test]
fn a_member_assembly_is_placed_in_the_assembly_that_holds_it() {
    for (name, outer_rotation, outer_translation) in [
        ("Assembly_nested.ics", IDENTITY, [30.0, -20.0, 10.0]),
        (
            "Assembly_nested_moved.ics",
            [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            [100.0, 50.0, 10.0],
        ),
    ] {
        let result = decode(name);
        assert_eq!(count(&result, IronCadLossCode::AssemblyBoundByOrder), 1);
        assert_eq!(count(&result, IronCadLossCode::AssemblyFlattened), 0);
        assert_eq!(members(&result, None), ["Outer"]);
        assert_eq!(members(&result, Some("Outer")), ["PartC", "Inner"]);
        assert_eq!(members(&result, Some("Inner")), ["PartA", "PartB"]);
        assert_placement(
            placement(&result, "Outer"),
            outer_rotation,
            outer_translation,
        );
        assert_placement(placement(&result, "Inner"), IDENTITY, [0.0, 0.0, 10.0]);
        assert_placement(placement(&result, "PartA"), IDENTITY, [0.0; 3]);
        assert_placement(placement(&result, "PartB"), IDENTITY, [0.0, 0.0, 25.0]);
        assert_placement(placement(&result, "PartC"), IDENTITY, [0.0; 3]);
    }
}

#[test]
fn an_inch_scene_states_inch_units() {
    let result = decode("Block_inch.ics");
    let root = result
        .ir()
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .unwrap();
    let units = root.units.as_ref().unwrap();
    assert_eq!(units.length, cadmpeg_ir::design::units::UnitSymbol::In);
    assert_point_box(&result, [-50.8, -25.4, 0.0, 50.8, 25.4, 25.4]);
}
