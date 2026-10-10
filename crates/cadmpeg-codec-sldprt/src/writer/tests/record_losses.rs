// SPDX-License-Identifier: Apache-2.0
//! Individual records in written arenas must appear, charge loss, or refuse.
#![allow(clippy::unwrap_used)]

use crate::{loss::SldprtLossCode, SldprtCodec};
use cadmpeg_ir::{
    appearance::{Appearance, AppearanceBinding, AppearanceTarget},
    codec::write::{target::TargetRequest, EncodeInput, Encoder},
    geometry::{
        pcurve::{LinePcurve, Pcurve, PcurveGeometry},
        CurveGeometry, SolvedCurveGeometry,
    },
    math::{Point2, Point3},
    topology::{BodyKind, Color},
    CadIr,
};

fn cube() -> CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    ir.model.bodies[0].name = None;
    for face in &mut ir.model.faces {
        face.name = None;
    }
    for edge in &mut ir.model.edges {
        edge.set_param_range(None);
    }
    ir
}

fn appearance(schema: Option<&str>) -> Appearance {
    Appearance {
        id: "test:model:appearance#asset".try_into().unwrap(),
        name: Some("test".into()),
        asset_guid: None,
        visual_guid: None,
        physical_token: None,
        library_id: None,
        schema: schema.map(str::to_owned),
        category: None,
        base_color: Some(Color::new(0.2, 0.3, 0.4, 1.0).unwrap()),
        properties: std::collections::BTreeMap::default(),
        textures: Vec::new(),
    }
}

fn assert_loss(ir: &CadIr, code: SldprtLossCode, prefix: &str) {
    let plan = SldprtCodec
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .unwrap();
    let loss = plan
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == code.kind())
        .unwrap();
    assert!(loss.message.starts_with(prefix), "{}", loss.message);
}

#[test]
fn attributes_outside_sldprt_namespace_have_counted_export_loss() {
    let mut ir = cube();
    ir.model
        .attributes
        .push(cadmpeg_ir::attributes::SourceAttribute {
            id: "test:model:attribute#other".try_into().unwrap(),
            target: cadmpeg_ir::attributes::AttributeTarget::Document {},
            name: "other".into(),
            values: vec![cadmpeg_ir::attributes::AttributeValue::String(
                "value".into(),
            )],
        });
    assert_loss(
        &ir,
        SldprtLossCode::WriterAttributesOmitted,
        "1 attribute outside the sldprt namespace record(s)",
    );
}

#[test]
fn edge_and_vertex_appearance_bindings_have_counted_export_loss() {
    for target in [
        AppearanceTarget::Edge(cube().model.edges[0].id.clone()),
        AppearanceTarget::Vertex(cube().model.vertices[0].id.clone()),
    ] {
        let mut ir = cube();
        let appearance = appearance(Some("moVisualProperties_c"));
        ir.model.appearance_bindings.push(AppearanceBinding {
            id: "test:model:appearance-binding#other".try_into().unwrap(),
            target,
            appearance: appearance.id.clone(),
            source_entity_id: None,
            object_type: None,
            visible: None,
            channels: std::collections::BTreeMap::default(),
        });
        ir.model.appearances.push(appearance);
        assert_loss(
            &ir,
            SldprtLossCode::WriterAppearanceBindingsOmitted,
            "1 appearance binding to a target other than a face or body record(s)",
        );
    }
}

#[test]
fn unbound_appearance_without_material_schema_has_counted_export_loss() {
    let mut ir = cube();
    ir.model.appearances.push(appearance(None));
    assert_loss(
        &ir,
        SldprtLossCode::WriterUnboundAppearancesOmitted,
        "1 unbound appearance outside moVisualProperties_c record(s)",
    );
    ir.model.appearances[0].schema = Some("moVisualProperties_c".into());
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == SldprtLossCode::WriterUnboundAppearancesOmitted.kind()));
}

#[test]
fn tessellation_body_faces_and_deflection_each_have_export_loss() {
    let mut ir = cube();
    let mut mesh = cadmpeg_ir::tessellation::Tessellation::new(
        "test:model:tessellation#mesh".try_into().unwrap(),
        cadmpeg_ir::tessellation::TessellationMesh::List {
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            triangles: vec![[0, 1, 2]],
        },
        Vec::new(),
    )
    .unwrap();
    mesh.body = Some(ir.model.bodies[0].id.clone());
    mesh.faces = vec![ir.model.faces[0].id.clone()];
    mesh.set_chordal_deflection(Some(0.001)).unwrap();
    ir.model.tessellations.push(mesh);
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    for field in ["body binding", "face binding", "chordal deflection"] {
        assert!(plan.report().losses.iter().any(|loss| loss.code
            == SldprtLossCode::WriterTessellationMetadataOmitted.kind()
            && loss
                .message
                .starts_with(&format!("1 tessellation {field} record(s)"))));
    }
}

#[test]
fn generated_wire_and_general_bodies_refuse_solid_lump_conversion() {
    for kind in [BodyKind::Wire, BodyKind::General] {
        let mut ir = cube();
        ir.model.bodies[0].kind = kind;
        let error = crate::writer::brep_body(&ir, 0.001, false).unwrap_err();
        assert!(error.to_string().contains("as a solid lump"));
    }
}

#[test]
fn regenerated_shell_wire_members_and_pcurve_uses_have_counted_losses() {
    let mut ir = cube();
    let shell = &ir.model.shells[0];
    ir.model.shells[0] = cadmpeg_ir::topology::Shell::new(
        shell.id.clone(),
        shell.region.clone(),
        shell.faces().to_vec(),
        vec![ir.model.edges[0].id.clone()],
        vec![ir.model.vertices[0].id.clone()],
    )
    .unwrap();
    let pcurve = Pcurve {
        id: "test:model:pcurve#use".try_into().unwrap(),
        geometry: PcurveGeometry::Line(
            LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)).unwrap(),
        ),
        metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::default(),
    };
    ir.model.coedges[0]
        .pcurves
        .push(cadmpeg_ir::topology::PcurveUse {
            pcurve: pcurve.id.clone(),
            isoparametric: None,
            parameter_range: None,
        });
    ir.model.pcurves.push(pcurve);
    // The topology writer ignores the shell members and the coedge relation;
    // accounting belongs to that write, irrespective of neutral admission.
    crate::writer::brep_body(&ir, 0.001, false).unwrap();
    let losses = super::super::accounting::losses(&ir, true);
    for (code, field) in [
        (
            SldprtLossCode::WriterShellWireTopologyOmitted,
            "shell wire edge ownership",
        ),
        (
            SldprtLossCode::WriterShellWireTopologyOmitted,
            "shell free vertex ownership",
        ),
        (
            SldprtLossCode::WriterCoedgePcurveUsesOmitted,
            "coedge pcurve use",
        ),
    ] {
        assert!(losses.iter().any(|loss| loss.code == code.kind()
            && loss.message.starts_with(&format!("1 {field} record(s)"))));
    }
    let retained_losses = super::super::accounting::losses(&ir, false);
    assert!(!retained_losses.iter().any(|loss| loss.code
        == SldprtLossCode::WriterShellWireTopologyOmitted.kind()
        || loss.code == SldprtLossCode::WriterCoedgePcurveUsesOmitted.kind()));
}

#[test]
fn derived_degenerate_sphere_seam_curve_has_counted_export_loss() {
    let mut ir = cube();
    let mut curve = ir.model.curves[0].clone();
    curve.id = "sldprt:brep:curve#sphere-seam-face:synthetic"
        .try_into()
        .unwrap();
    curve.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(
        cadmpeg_ir::geometry::analytic::DegenerateCurve::try_new(Point3::new(0.0, 0.0, 0.0))
            .unwrap(),
    ));
    curve.source_object = Some(cadmpeg_ir::SourceObjectAssociation {
        format: cadmpeg_ir::CodecFormat::Sldprt,
        geometry_role: Some(cadmpeg_ir::SourceGeometryRole::Support),
        object_id: cadmpeg_core::nonblank_literal!("sphere-seam"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    });
    ir.model.curves.push(curve);
    assert_loss(
        &ir,
        SldprtLossCode::WriterSphereSeamCurvesOmitted,
        "1 degenerate sphere-seam curve record(s)",
    );
}

#[test]
fn opaque_carriers_require_a_retained_partition_before_generated_accounting() {
    let mut ir = cube();
    ir.model.curves[0].geometry =
        CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None });
    let error = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .err()
        .unwrap();
    assert!(error
        .to_string()
        .contains("without a retained native partition"));
}
