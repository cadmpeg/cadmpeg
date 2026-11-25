// SPDX-License-Identifier: Apache-2.0
//! A cage body is written as an `ON_SubD` object; as a mesh of the cage's
//! control net that carries the cage as `ON_SubDMeshProxyUserData` where the
//! archive holds no `ON_SubD` object (§7.2.6); or, for a T-spline cage at
//! archive version 50, as a mesh of the control net only.

use std::io::Cursor;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::report::export::ExportReport;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::CadIr;
use cadmpeg_test_support::cages::{push_cage, push_cage_body, CageBody};

use super::planar_tetrahedron;
use crate::test_support::{start_sharpness_cage, two_quad_cage};
use crate::{RhinoArchiveVersion, RhinoCodec};

fn write(ir: &CadIr, version: RhinoArchiveVersion) -> (ExportReport, CadIr) {
    let (report, decoded, _) = write_and_read(ir, version);
    (report, decoded)
}

/// The export report, the decoded document, and the messages of the decode
/// losses and notes.
fn write_and_read(ir: &CadIr, version: RhinoArchiveVersion) -> (ExportReport, CadIr, Vec<String>) {
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
    let messages = decoded
        .report()
        .losses
        .iter()
        .map(|loss| loss.message.clone())
        .chain(decoded.report().notes.iter().cloned())
        .collect();
    (report, decoded.ir().clone(), messages)
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

/// A cage body written as a mesh of the cage's control net, with a loss that
/// names the cage and its body.
fn assert_control_mesh(report: &ExportReport, decoded: &CadIr, cage: &CageBody) {
    assert!(
        report.losses.iter().any(|loss| {
            loss.code.local_code() == crate::loss::RhinoLossCode::WriterSubdControlMesh.code()
                && loss.message.contains(cage.subd.as_str())
                && loss.message.contains(cage.body.as_str())
        }),
        "{:#?}",
        report.losses
    );
    assert!(decoded.geometry.subds.is_empty());
    assert_eq!(decoded.geometry.bodies.len(), 1);
    assert_eq!(decoded.model.tessellations.len(), 1);
    let points = two_quad_cage()
        .vertices()
        .iter()
        .map(|vertex| vertex.point().get())
        .collect::<Vec<_>>();
    let written = decoded.model.tessellations[0]
        .vertices()
        .into_iter()
        .map(FinitePoint3::get)
        .collect::<Vec<_>>();
    assert_eq!(written, points);
}

#[test]
fn an_archive_50_cage_body_is_written_as_a_subd_mesh_proxy() {
    let mut ir = planar_tetrahedron();
    let cage = push_cage(&mut ir, "rhino", two_quad_cage());

    let (report, decoded) = write(&ir, RhinoArchiveVersion::V5);
    let cage_losses = report
        .losses
        .iter()
        .filter(|loss| loss.message.contains(cage.subd.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(cage_losses.len(), 1, "{:#?}", report.losses);
    assert!(cage_losses[0].message.contains("start sharpness"));
    assert!(decoded.model.tessellations.is_empty());
    assert_eq!(decoded.geometry.bodies.len(), 2);
    assert_eq!(decoded.geometry.subds.len(), 1);
    assert_eq!(
        decoded.geometry.subds[0].cage,
        start_sharpness_cage(&two_quad_cage())
    );
}

/// A document of the planar tetrahedron and a T-spline cage body of
/// `two_quad_cage`, named and colored.
fn t_spline_cage_body() -> (CadIr, CageBody, Color) {
    let mut ir = planar_tetrahedron();
    let cage = push_cage(&mut ir, "rhino", two_quad_cage());
    ir.geometry.subds[0].scheme = cadmpeg_ir::SubdScheme::TSpline;
    let color = Color::new(1.0, 128.0 / 255.0, 0.0, 1.0).expect("valid color");
    let body = ir
        .geometry
        .bodies
        .iter_mut()
        .find(|body| body.id == cage.body)
        .expect("the cage body");
    body.name = Some("t-spline".into());
    body.color = Some(color);
    (ir, cage, color)
}

#[test]
fn a_t_spline_cage_body_is_written_as_a_catmull_clark_subd_object() {
    for (version, expected) in [
        (
            RhinoArchiveVersion::V7,
            start_sharpness_cage(&two_quad_cage()),
        ),
        (RhinoArchiveVersion::V8, two_quad_cage()),
    ] {
        let (ir, cage, color) = t_spline_cage_body();

        let (report, decoded) = write(&ir, version);
        assert!(
            report.losses.iter().any(|loss| {
                loss.code.local_code() == crate::loss::RhinoLossCode::WriterTsplineReduced.code()
                    && loss.message.contains(cage.subd.as_str())
                    && loss.message.contains(cage.body.as_str())
                    && loss.message.contains("knot intervals")
            }),
            "{:#?}",
            report.losses
        );
        assert_eq!(decoded.geometry.bodies.len(), 2, "{version:?}");
        assert_eq!(decoded.geometry.subds.len(), 1, "{version:?}");
        assert_eq!(decoded.geometry.subds[0].cage, expected, "{version:?}");
        let written = decoded
            .geometry
            .bodies
            .iter()
            .find(|body| body.name.as_deref() == Some("t-spline"))
            .expect("the cage body keeps its name");
        assert_eq!(written.color, Some(color));
    }
}

#[test]
fn an_archive_50_t_spline_cage_body_is_written_as_its_control_net() {
    let (ir, cage, _) = t_spline_cage_body();

    let (report, decoded) = write(&ir, RhinoArchiveVersion::V5);
    assert_control_mesh(&report, &decoded, &cage);
}

#[test]
fn a_body_of_faces_and_a_cage_writes_an_object_id_for_each() {
    let mut ir = planar_tetrahedron();
    let cage = push_cage(&mut ir, "rhino", two_quad_cage());
    let owner = ir.geometry.bodies[0].id.clone();
    ir.geometry.bodies.retain(|body| body.id != cage.body);
    for definition in &mut ir.model.product_definitions {
        definition.bodies.retain(|body| *body != cage.body);
    }
    for region in &mut ir.geometry.regions {
        if region.body == cage.body {
            region.body = owner.clone();
        }
    }

    let (report, decoded, messages) = write_and_read(&ir, RhinoArchiveVersion::V8);
    assert!(
        messages
            .iter()
            .all(|message| !message.contains("duplicate object UUID")),
        "{messages:#?}"
    );
    assert!(
        report.losses.iter().any(|loss| {
            loss.code.local_code() == crate::loss::RhinoLossCode::WriterBodySplit.code()
                && loss.message.contains(owner.as_str())
                && loss.message.contains(cage.subd.as_str())
        }),
        "{:#?}",
        report.losses
    );
    assert_eq!(decoded.geometry.subds.len(), 1);
    assert_eq!(decoded.geometry.faces.len(), 4);
}

#[test]
fn a_cage_of_no_face_is_named_where_the_archive_holds_no_subd() {
    let mut ir = planar_tetrahedron();
    let cage = push_cage_body(&mut ir, "rhino");

    let (report, decoded) = write(&ir, RhinoArchiveVersion::V5);
    assert!(
        report
            .losses
            .iter()
            .any(|loss| loss.message.contains(cage.subd.as_str())
                && loss.message.contains(cage.body.as_str())),
        "{:#?}",
        report.losses
    );
    assert_eq!(decoded.geometry.bodies.len(), 1);
}
