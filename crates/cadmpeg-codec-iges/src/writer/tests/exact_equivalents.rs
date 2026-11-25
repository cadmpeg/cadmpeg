// SPDX-License-Identifier: Apache-2.0
//! Documents that IGES states through an exact equivalent are written and
//! read back.

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::{add_document_root, unit_cube};
use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::native::NativeRecord;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::BodyKind;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::FiniteVector;
use cadmpeg_ir::CadIr;

use super::super::native_record_losses;
use super::super::pcurve_entity;
use super::super::records::TopologyIndex;
use crate::{IgesCodec, IgesVersion};

const OPEN_SHELL: &[u8] = include_bytes!("../../../tests/golden/fixtures/explicit_open_shell.igs");
const LOOPLESS_FACE: &[u8] =
    include_bytes!("../../../tests/golden/fixtures/parameter_domain_trimmed_surface.igs");

fn decoded(bytes: &[u8]) -> CadIr {
    IgesCodec
        .decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
        .expect("the file decodes")
        .ir()
        .clone()
}

fn written(ir: &CadIr, version: IgesVersion) -> (Vec<LossNote>, Vec<u8>) {
    let plan = IgesCodec
        .plan(
            EncodeInput::new(ir, None)
                .with_target(TargetRequest::Explicit(version.descriptor().id.as_str())),
        )
        .expect("the document is written");
    let losses = plan.report().losses.clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("the file is written");
    (losses, bytes)
}

fn charged<'a>(losses: &'a [LossNote], code: &str) -> Vec<&'a str> {
    losses
        .iter()
        .filter(|loss| loss.code.to_string() == code)
        .map(|loss| loss.message.as_str())
        .collect()
}

/// The (type, form) of each Directory Entry, in file order.
fn directory(bytes: &[u8]) -> Vec<(i64, i64)> {
    let text = std::str::from_utf8(bytes).expect("IGES Fixed ASCII is UTF-8");
    let cards = text
        .lines()
        .filter(|card| card.as_bytes().get(72) == Some(&b'D'))
        .collect::<Vec<_>>();
    cards
        .chunks(2)
        .map(|pair| {
            let field = |card: &str, start: usize| {
                card[start..start + 8]
                    .trim()
                    .parse::<i64>()
                    .expect("a numeric Directory field")
            };
            (field(pair[0], 0), field(pair[1], 32))
        })
        .collect()
}

fn cube_with(rows: [[f64; 4]; 3]) -> CadIr {
    let mut ir = unit_cube().expect("the example cube builds");
    add_document_root(&mut ir);
    ir.geometry.bodies[0].transform = Transform::affine(rows);
    ir
}

fn assert_rows_eq(actual: Option<Transform>, expected: [[f64; 4]; 3]) {
    let actual = actual
        .expect("the body reads back with a transform")
        .affine_rows();
    for (actual, expected) in actual.iter().flatten().zip(expected.iter().flatten()) {
        assert!(
            (actual - expected).abs() <= 1.0e-12,
            "{actual} != {expected}"
        );
    }
}

#[test]
fn a_translated_solid_writes_a_type_124_form_0_and_reads_back_placed() {
    let rows = [
        [0.0, -1.0, 0.0, 5.0],
        [1.0, 0.0, 0.0, -2.0],
        [0.0, 0.0, 1.0, 3.0],
    ];
    let (losses, bytes) = written(&cube_with(rows), IgesVersion::V5_3);
    assert!(
        charged(&losses, "iges/writer.body-omitted").is_empty(),
        "{losses:#?}"
    );
    let entries = directory(&bytes);
    assert!(entries.contains(&(124, 0)), "{entries:?}");
    assert!(entries.contains(&(186, 0)), "{entries:?}");
    let read = decoded(&bytes);
    assert_eq!(read.geometry.bodies.len(), 1);
    assert_rows_eq(read.geometry.bodies[0].transform, rows);
}

#[test]
fn a_mirrored_solid_writes_a_type_124_form_1_and_reads_back_placed() {
    let rows = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, -1.0, 4.0],
    ];
    let (losses, bytes) = written(&cube_with(rows), IgesVersion::V5_3);
    assert!(
        charged(&losses, "iges/writer.body-omitted").is_empty(),
        "{losses:#?}"
    );
    assert!(directory(&bytes).contains(&(124, 1)));
    let read = decoded(&bytes);
    assert_eq!(read.geometry.bodies.len(), 1);
    assert_rows_eq(read.geometry.bodies[0].transform, rows);
}

#[test]
fn a_sheet_body_transform_drops_the_body_naming_the_solid_rule() {
    let mut ir = decoded(OPEN_SHELL);
    let sheet = ir
        .geometry
        .bodies
        .iter_mut()
        .find(|body| body.kind == BodyKind::Sheet)
        .expect("the fixture holds a sheet body");
    sheet.transform = Transform::affine([
        [1.0, 0.0, 0.0, 5.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ]);
    let id = sheet.id.to_string();
    let (losses, _) = written(&ir, IgesVersion::V5_3);
    let omitted = charged(&losses, "iges/writer.body-omitted");
    assert_eq!(omitted.len(), 1, "{losses:#?}");
    assert!(omitted[0].contains(&id), "{omitted:?}");
    assert!(
        omitted[0].contains("Type 124 on a Type 186 solid"),
        "{omitted:?}"
    );
}

#[test]
fn a_plane_face_without_parameter_curves_writes_computed_ones() {
    let ir = decoded(OPEN_SHELL);
    assert!(
        ir.geometry.pcurves.is_empty(),
        "the fixture states no pcurve"
    );
    let (losses, bytes) = written(&ir, IgesVersion::V5_3);
    assert!(
        charged(&losses, "iges/writer.face-omitted").is_empty(),
        "{losses:#?}"
    );
    let entries = directory(&bytes);
    assert!(entries.iter().any(|entry| entry.0 == 144), "{entries:?}");
    assert!(entries.iter().any(|entry| entry.0 == 142), "{entries:?}");
    let read = decoded(&bytes);
    assert_eq!(read.geometry.faces.len(), 1);
    assert!(!read.geometry.coedges.is_empty());
    assert!(read
        .geometry
        .coedges
        .iter()
        .all(|coedge| !coedge.pcurves.is_empty()));
}

#[test]
fn a_face_without_a_boundary_loop_writes_an_untrimmed_type_144() {
    let ir = decoded(LOOPLESS_FACE);
    assert_eq!(ir.geometry.faces.len(), 1);
    assert_eq!(ir.geometry.faces[0].loops.stated().count(), 0);
    let (losses, bytes) = written(&ir, IgesVersion::V5_3);
    assert!(
        charged(&losses, "iges/writer.face-omitted").is_empty(),
        "{losses:#?}"
    );
    assert!(
        charged(&losses, "export/output.empty").is_empty(),
        "{losses:#?}"
    );
    assert!(directory(&bytes).contains(&(144, 0)));
    let read = decoded(&bytes);
    assert_eq!(read.geometry.faces.len(), 1);
    assert_eq!(read.geometry.faces[0].loops.stated().count(), 0);
}

#[test]
fn a_line_pcurve_writes_as_a_degree_1_type_126() {
    test_ctx!(ctx);
    let pcurve = Pcurve {
        id: "test:model:pcurve#line".try_into().expect("valid identity"),
        geometry: PcurveGeometry::Line(
            LinePcurve::try_new(Point2::new(1.0, 0.0), Point2::new(0.0, 1.0))
                .expect("a unit direction"),
        ),
        metadata: PcurveMetadata::general(None, FiniteVector::new([0.0, 2.0]), None),
        native_ref: None,
    };
    let mut ir = CadIr::empty();
    ir.geometry.pcurves.push(pcurve.clone());
    let index = TopologyIndex::new(&ir);
    let entity = pcurve_entity(&ctx, &index, &pcurve).expect("a line pcurve is written");
    assert_eq!(entity.type_code, 126);
    let parameters = String::from_utf8(entity.parameter_body).expect("ASCII parameters");
    assert!(parameters.starts_with("1,1,"), "{parameters}");
}

#[test]
fn a_native_curve_without_a_directory_sequence_names_that_cause() {
    let mut ir = CadIr::empty();
    let mut fields = serde_json::Map::new();
    fields.insert("entity_type".into(), 126.into());
    fields.insert("form".into(), 0.into());
    let record = NativeRecord::new(
        "iges:native:entity#unsequenced"
            .try_into()
            .expect("valid identity"),
        fields,
    )
    .expect("a flat record");
    ir.native
        .namespace_mut(cadmpeg_ir::codec_id!("iges"))
        .set_arena_records("entities", vec![record])
        .expect("a valid arena name");
    let messages = native_record_losses(&ir)
        .into_iter()
        .map(|loss| loss.message)
        .collect::<Vec<_>>();
    assert!(
        messages.contains(
            &"the IGES write omits 1 native record(s) whose record states no Directory sequence that ties it to a neutral geometry record: iges:native:entity#unsequenced"
                .to_owned()
        ),
        "{messages:#?}"
    );
}
