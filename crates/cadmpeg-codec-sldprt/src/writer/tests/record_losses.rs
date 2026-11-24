// SPDX-License-Identifier: Apache-2.0
//! Individual records in written arenas must appear, charge loss, or refuse.
#![allow(clippy::unwrap_used)]

use crate::{loss::SldprtLossCode, SldprtCodec};
use cadmpeg_ir::source_fidelity::CodecDigests;
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
    ir.geometry.bodies[0].name = None;
    for face in &mut ir.geometry.faces {
        face.name = None;
    }
    for edge in &mut ir.geometry.edges {
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
        native_ref: None,
    }
}

fn assert_loss(ir: &CadIr, code: SldprtLossCode, prefix: &str) {
    let plan = crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(ir, None),
        TargetRequest::Inherit,
    )
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
            target: cadmpeg_ir::attributes::AttributeTarget::Document {}.into(),
            name: "other".into(),
            values: vec![Some(cadmpeg_ir::attributes::AttributeValue::String(
                "value".into(),
            ))],
            native_ref: None,
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
        AppearanceTarget::Edge(cube().geometry.edges[0].id.clone()),
        AppearanceTarget::Vertex(cube().geometry.vertices[0].id.clone()),
    ] {
        let mut ir = cube();
        let appearance = appearance(Some("moVisualProperties_c"));
        ir.model.appearance_bindings.push(AppearanceBinding {
            id: "test:model:appearance-binding#other".try_into().unwrap(),
            target: target.into(),
            appearance: appearance.id.clone(),
            precedence: None,
            source_entity_id: None,
            object_type: None,
            visible: None,
            channels: std::collections::BTreeMap::default(),
            native_ref: None,
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
    let plan = crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(&ir, None),
        TargetRequest::Inherit,
    )
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
    mesh.of = cadmpeg_ir::tessellation::Meshed::Body {
        body: ir.geometry.bodies[0].id.clone(),
        faces: vec![ir.geometry.faces[0].id.clone()],
    };
    mesh.set_chordal_deflection(Some(0.001)).unwrap();
    ir.model.tessellations.push(mesh);
    let plan = crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(&ir, None),
        TargetRequest::Inherit,
    )
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
        ir.geometry.bodies[0].kind = kind;
        let error = crate::writer::brep_body(&ir, 0.001, false).unwrap_err();
        assert!(error.to_string().contains("as a solid lump"));
    }
}

#[test]
fn regenerated_shell_wire_members_and_pcurve_uses_have_counted_losses() {
    let mut ir = cube();
    let shell = &ir.geometry.shells[0];
    ir.geometry.shells[0] = cadmpeg_ir::topology::Shell::with_members(
        shell.id.clone(),
        shell.region.clone(),
        vec![ir.geometry.edges[0].id.clone()],
        vec![ir.geometry.vertices[0].id.clone()],
    );
    let pcurve = Pcurve {
        id: "test:model:pcurve#use".try_into().unwrap(),
        geometry: PcurveGeometry::Line(
            LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)).unwrap(),
        ),
        metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::default(),
        native_ref: None,
    };
    ir.geometry.coedges[0]
        .pcurves
        .push(cadmpeg_ir::topology::PcurveUse {
            pcurve: pcurve.id.clone(),
            isoparametric: None,
            parameter_range: None,
        });
    ir.geometry.pcurves.push(pcurve);
    // The topology writer ignores the shell members and the coedge relation;
    // accounting belongs to that write, irrespective of neutral admission.
    crate::writer::brep_body(&ir, 0.001, false).unwrap();
    let losses = super::super::accounting::losses(&ir, &CodecDigests::new(), true);
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
    let retained_losses = super::super::accounting::losses(&ir, &CodecDigests::new(), false);
    assert!(!retained_losses.iter().any(|loss| loss.code
        == SldprtLossCode::WriterShellWireTopologyOmitted.kind()
        || loss.code == SldprtLossCode::WriterCoedgePcurveUsesOmitted.kind()));
}

#[test]
fn derived_degenerate_sphere_seam_curve_has_counted_export_loss() {
    let mut ir = cube();
    let mut curve = ir.geometry.curves[0].clone();
    curve.id = "sldprt:brep:curve#sphere-seam-face:synthetic"
        .try_into()
        .unwrap();
    curve.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(
        cadmpeg_ir::geometry::analytic::DegenerateCurve::try_new(Point3::new(0.0, 0.0, 0.0))
            .unwrap(),
    ));
    curve.source_object = Some(cadmpeg_ir::SourceObjectAssociation {
        format: cadmpeg_ir::codec_id!("sldprt"),
        geometry_role: Some(cadmpeg_ir::SourceGeometryRole::Support),
        object_id: cadmpeg_core::nonblank_literal!("sphere-seam"),
        instance_path: Vec::new(),
    });
    ir.geometry.curves.push(curve);
    assert_loss(
        &ir,
        SldprtLossCode::WriterSphereSeamCurvesOmitted,
        "1 degenerate sphere-seam curve record(s)",
    );
}

#[test]
fn a_body_on_an_opaque_carrier_without_a_retained_partition_is_omitted() {
    let mut ir = cube();
    ir.geometry.curves[0].geometry =
        CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None });
    let curve = ir.geometry.curves[0].id.as_str().to_owned();
    let edge = ir
        .geometry
        .edges
        .iter()
        .find(|edge| edge.curve().is_some_and(|id| id.as_str() == curve))
        .unwrap()
        .id
        .as_str()
        .to_owned();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(plan.report().losses.iter().any(|loss| {
        loss.code.local_code() == "writer.bodies-omitted"
            && loss.message.contains(&format!("edge {edge}"))
    }));
}

#[test]
fn native_skeleton_losses_name_summarized_features_and_an_orphan_lane() {
    let codes = |losses: &[cadmpeg_ir::report::loss::LossNote]| {
        losses
            .iter()
            .map(|loss| loss.code.clone())
            .collect::<Vec<_>>()
    };
    let bare = crate::writer::native_skeleton_losses(None, None, false, &[], None);
    assert_eq!(
        codes(&bare),
        [
            SldprtLossCode::WriterNativeSkeletonUnlaidRuns.kind(),
            SldprtLossCode::WriterNativeStreamsOmitted.kind(),
        ]
    );
    let full = crate::writer::native_skeleton_losses(
        Some((
            "Contents/Keywords-0".to_owned(),
            vec!["feature#a".to_owned(), "feature#b".to_owned()],
            None,
        )),
        Some("sketch#c holds an ellipse".to_owned()),
        true,
        &["Disc".to_owned()],
        Some(&[crate::writer::native::from_ir::SourceFeature {
            id: "feature#coil".to_owned(),
            outputs: vec!["body#coil".to_owned()],
        }]),
    );
    assert_eq!(
        codes(&full),
        [
            SldprtLossCode::WriterNativeHistorySummary.kind(),
            SldprtLossCode::WriterNativeSkeletonUnlaidRuns.kind(),
            SldprtLossCode::WriterNativeStreamsOmitted.kind(),
            SldprtLossCode::WriterNativeSketchLaneOrphan.kind(),
            SldprtLossCode::WriterNativeSketchProfileBodyKept.kind(),
            SldprtLossCode::WriterNativeSketchOriginRelationAdded.kind(),
            SldprtLossCode::WriterBodiesOmitted.kind(),
            SldprtLossCode::WriterNativeBodyIdentityOmitted.kind(),
        ]
    );
    assert!(full[0].message.starts_with(
        "2 source feature(s) are written only in the cadmpeg history summary Contents/Keywords-0,"
    ));
    assert!(full[0].message.ends_with("feature#a, feature#b"));
    let (runs, bytes) = crate::writer::native::detailing_zero_runs();
    assert!(full[1].message.contains(&format!(
        "{runs} runs ({bytes} bytes) of the moRelMgr_c detailing settings"
    )));
    assert!(full[2].message.contains("Contents/3DExperienceExchange2"));
    assert!(full[3].message.ends_with("sketch#c holds an ellipse"));
    assert!(full[4]
        .message
        .contains("a native top-level sketch writes flag byte 1 and no body"));
    assert!(full[5]
        .message
        .ends_with("a relation the document does not state: Disc"));
    assert!(full[6].message.ends_with("feature#coil made body#coil"));
    assert!(full[7].message.contains("FACE_ID_2001"));
    assert!(full[7]
        .message
        .contains("moBBoxCenterData_c writes a zero point, a zero length and identifier 0"));
    assert!(full[1].message.contains("each native sketch's 76-byte"));
    assert!(!bare[0].message.contains("each native sketch's 76-byte"));
}

/// A driven dimension of a generated sketch and its parameter, which no
/// record of the write holds, are each charged.
#[test]
fn a_driven_dimension_of_a_generated_sketch_has_a_counted_export_loss() {
    use cadmpeg_ir::sketches::{
        SketchConstraint, SketchConstraintDefinition, SketchConstraintDefinitionInput,
        SketchConstraintId, SketchEntityId, SketchId, SketchLocus,
    };
    let mut ir = cube();
    let point = |id: &str| SketchLocus::Entity(SketchEntityId::mint(id).unwrap());
    ir.model
        .parameters
        .push(cadmpeg_ir::features::parameters::DesignParameter {
            product_definition: None,
            id: cadmpeg_ir::features::ParameterId::mint("synthetic:test:id#width").unwrap(),
            owner: None,
            ordinal: 0,
            name: "D1".to_owned(),
            expression: "2mm".to_owned(),
            display: None,
            value: None,
            dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
    ir.model.sketch_constraints.push(SketchConstraint {
        id: SketchConstraintId::mint("synthetic:test:id#reference-dimension").unwrap(),
        sketch: SketchId::mint("synthetic:test:id#sketch").unwrap(),
        definition: SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::DistanceLoci {
                first: point("synthetic:test:id#first"),
                second: point("synthetic:test:id#second"),
                parameter: cadmpeg_ir::features::ParameterId::mint("synthetic:test:id#width")
                    .unwrap(),
            },
        )
        .unwrap(),
        name: None,
        driving: Some(false),
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    });
    let losses = super::super::accounting::losses(&ir, &CodecDigests::new(), false);
    for prefix in [
        "1 driven (reference) sketch dimension record(s)",
        "1 driven (reference) dimension parameter record(s)",
    ] {
        assert!(
            losses.iter().any(|loss| loss.code
                == SldprtLossCode::WriterDrivenDimensionsOmitted.kind()
                && loss.message.starts_with(prefix)),
            "{prefix}: {losses:?}"
        );
    }

    // A parameter whose expression reads the driven one is written with its
    // value, and one with no value is left out. A write that reduces a
    // foreign history names those where it writes every expression so; a
    // write that keeps the source's history names them here.
    let reader = |key: &str, name: &str, value: Option<f64>| {
        let mut reader = ir.model.parameters[0].clone();
        reader.id =
            cadmpeg_ir::features::ParameterId::mint(format!("synthetic:test:id#{key}")).unwrap();
        reader.name = name.to_owned();
        reader.expression = "\"D1\" * 2".to_owned();
        reader.value = value.map(|millimetres| {
            cadmpeg_ir::features::parameters::ParameterValue::Length(
                cadmpeg_ir::scalar::Length::new(millimetres).unwrap(),
            )
        });
        reader.dependencies = [ir.model.parameters[0].id.clone()].into_iter().collect();
        reader
    };
    let twice = reader("twice", "D2", Some(4.0));
    let unknown = reader("unknown", "D3", None);
    ir.model.parameters.extend([twice, unknown]);
    let named = |ir: &CadIr, digests: &CodecDigests| {
        super::super::accounting::losses(ir, digests, false)
            .into_iter()
            .filter(|loss| loss.message.contains("reads an omitted"))
            .map(|loss| (loss.code, loss.message))
            .collect::<Vec<_>>()
    };
    assert!(named(&ir, &CodecDigests::new()).is_empty());
    let named = named(
        &ir,
        &CodecDigests::from([(
            cadmpeg_core::nonblank_literal!("sldprt_native_history_sha256"),
            cadmpeg_ir::hash::digest::Sha256Digest::digest(b"history"),
        )]),
    );
    assert!(
        named.iter().any(|(code, message)| *code
            == SldprtLossCode::WriterParameterExpressionsOmitted.kind()
            && message.ends_with("dimension parameter: synthetic:test:id#twice")),
        "{named:?}"
    );
    assert!(
        named.iter().any(
            |(code, message)| *code == SldprtLossCode::WriterFeaturesOmitted.kind()
                && message.ends_with("write in its place: synthetic:test:id#unknown")
        ),
        "{named:?}"
    );
}
