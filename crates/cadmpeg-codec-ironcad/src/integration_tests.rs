// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, InspectOptions};
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions, DecodeResult};

use crate::loss::IronCadLossCode;
use crate::test_support::corpus;
use crate::IronCadCodec;

const BOX: &str = "Box_50x40x25.ics";
const CYLINDER: &str = "Cylinder_50x50.ics";
const SCENE: &str = "Ironcad_2025_simple_test_parts.ics";
const ACIS_BLOCK: &str = "Block_ACIS_100x50x20.ics";
/// Agreement of a decoded corner with the modelled block dimensions.
const EPS_POINT_MM: f64 = 1.0e-9;
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
        assert_eq!(count(&result, IronCadLossCode::BodyUnreadable), 0, "{name}");
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
    assert_eq!(count(&result, IronCadLossCode::BodyUnreadable), 0);
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
    assert_eq!(count(&result, IronCadLossCode::BodyUnreadable), 0);
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
        let records = crate::tree::read(payload).unwrap();
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
    assert_eq!(count(&result, IronCadLossCode::AssemblyFlattened), 1);
    let model = &result.ir().model;
    let names: Vec<_> = model
        .occurrences
        .iter()
        .map(|occurrence| occurrence.name.as_deref().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "Box_Parasolid",
            "Cylinder_ACIS",
            "Compound_part",
            "Part6",
            "Part4"
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
    assert_eq!(root.occurrences.len(), 5);
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
    assert_eq!(count(&result, IronCadLossCode::UnitsFirstStated), 1);
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
    assert_eq!(model.mass_properties.len(), 5);
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
