// SPDX-License-Identifier: Apache-2.0
//! A cage body is written as an `ON_SubD` object, and a cage that the archive
//! version cannot hold is left out with its body; the loss that names the
//! cage names the body (§5.6).

use std::io::Cursor;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::export::ExportReport;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::CadIr;
use cadmpeg_test_support::cages::{assert_cage_body_dropped, push_cage, push_cage_body};

use super::planar_tetrahedron;
use crate::test_support::two_quad_cage;
use crate::{RhinoArchiveVersion, RhinoCodec};

fn write(ir: &CadIr, version: RhinoArchiveVersion) -> (ExportReport, CadIr) {
    let mut bytes = Vec::new();
    let report = RhinoCodec
        .plan(
            EncodeInput::new(ir, None),
            TargetRequest::Explicit(version.descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("the write succeeds");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the written archive decodes");
    (report, decoded.ir().clone())
}

#[test]
fn a_cage_body_is_written_as_a_subd_object() {
    let mut ir = planar_tetrahedron();
    let cage = push_cage(&mut ir, "rhino", two_quad_cage());
    let color = Color::new(0.0, 128.0 / 255.0, 1.0, 1.0).expect("valid color");
    let body = ir
        .geometry
        .bodies
        .iter_mut()
        .find(|body| body.id == cage.body)
        .expect("the cage body");
    body.name = Some("cage".into());
    body.color = Some(color);

    let (report, decoded) = write(&ir, RhinoArchiveVersion::V8);
    assert!(
        report
            .losses
            .iter()
            .all(|loss| !loss.message.contains(cage.subd.as_str())),
        "{:#?}",
        report.losses
    );
    assert_eq!(report.census.counts.get("bodies"), Some(&2));
    assert_eq!(decoded.geometry.bodies.len(), 2);
    assert_eq!(decoded.geometry.subds.len(), 1);
    assert_eq!(decoded.geometry.subds[0].cage, two_quad_cage());
    let written = decoded
        .geometry
        .bodies
        .iter()
        .find(|body| body.name.as_deref() == Some("cage"))
        .expect("the cage body keeps its name");
    assert_eq!(written.color, Some(color));
}

#[test]
fn an_archive_70_cage_reports_its_end_sharpness() {
    let mut ir = planar_tetrahedron();
    let cage = push_cage(&mut ir, "rhino", two_quad_cage());

    let (report, decoded) = write(&ir, RhinoArchiveVersion::V7);
    assert!(
        report.losses.iter().any(|loss| {
            loss.message.contains(cage.subd.as_str()) && loss.message.contains("start sharpness")
        }),
        "{:#?}",
        report.losses
    );
    assert_eq!(decoded.geometry.subds.len(), 1);
    assert_eq!(decoded.geometry.bodies.len(), 2);
}

#[test]
fn an_archive_50_cage_body_is_left_out_with_its_cage() {
    let mut ir = planar_tetrahedron();
    let cage = push_cage_body(&mut ir, "rhino");

    let (report, decoded) = write(&ir, RhinoArchiveVersion::V5);
    assert_cage_body_dropped(&report, &cage, "bodies", 1);
    assert_eq!(decoded.geometry.bodies.len(), 1);
}

#[test]
fn a_t_spline_cage_body_is_left_out_with_its_cage() {
    let mut ir = planar_tetrahedron();
    let cage = push_cage(&mut ir, "rhino", two_quad_cage());
    ir.geometry.subds[0].scheme = cadmpeg_ir::SubdScheme::TSpline;

    let (report, decoded) = write(&ir, RhinoArchiveVersion::V7);
    assert_cage_body_dropped(&report, &cage, "bodies", 1);
    assert_eq!(decoded.geometry.bodies.len(), 1);
    assert!(decoded.geometry.subds.is_empty());
}
