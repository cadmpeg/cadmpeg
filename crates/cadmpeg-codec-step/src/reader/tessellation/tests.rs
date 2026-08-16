// SPDX-License-Identifier: Apache-2.0
//! AP242 indexed tessellation tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]
#![allow(unused_imports)]

use std::fmt::Write as _;
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeMode, InspectOptions};
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use cadmpeg_ir::eval::{
    model_curve_point_by_id, model_surface_partials_by_id, model_surface_point_by_id, pcurve_uv,
};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, SurfaceId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::{LengthUnit, Units};
use cadmpeg_ir::CadIr;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::ids::StepIdentity;
use crate::loss::StepLossCode;
use crate::test_support::{decode_inline, export};
use crate::{
    write_step, StepCodec, StepError, StepSchema, StepUnsupportedPolicy, StepWriteOptions,
};

const EPS_SAME_POINT: f64 = 1.0e-12;

fn assert_point3_close(actual: Point3, expected: Point3) {
    assert!((actual.x - expected.x).abs() < EPS_SAME_POINT);
    assert!((actual.y - expected.y).abs() < EPS_SAME_POINT);
    assert!((actual.z - expected.z).abs() < EPS_SAME_POINT);
}

fn assert_vector3_close(actual: Vector3, expected: Vector3) {
    assert!((actual.x - expected.x).abs() < EPS_SAME_POINT);
    assert!((actual.y - expected.y).abs() < EPS_SAME_POINT);
    assert!((actual.z - expected.z).abs() < EPS_SAME_POINT);
}

#[test]
pub(crate) fn decode_transfers_ap242_one_based_tessellation_indices() {
    let bytes = include_bytes!("../../../tests/fixtures/ap242_tessellation.p21");
    let result = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP242 tessellation");

    assert_eq!(result.ir().model.tessellations.len(), 2);
    assert_eq!(result.ir().model.bodies.len(), 1);
    let mesh = &result.ir().model.tessellations[0];
    assert_eq!(mesh.vertices.len(), 3);
    assert!((mesh.vertices[1].x - 10.0).abs() < EPS_SAME_POINT);
    assert_eq!(mesh.triangles, [[0, 1, 2]]);
    assert_eq!(mesh.normals.len(), 3);
    assert_eq!(
        mesh.body.as_ref().map(cadmpeg_ir::ids::BodyId::as_str),
        Some("step:data:body#38")
    );
    let complex = result
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.ends_with("#7"))
        .unwrap();
    assert_eq!(complex.triangles, [[0, 1, 2], [2, 1, 3], [0, 1, 3]]);
    assert_point3_close(complex.vertices[0], Point3::new(10.0, 10.0, 0.0));
    assert_eq!(complex.normals.len(), 4);
    assert!((complex.normals[0].x - 1.0).abs() < EPS_SAME_POINT);
    assert!(result
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Tessellation(_)
        )));
    assert!(result
        .report()
        .notes
        .iter()
        .any(|note| note
            == "geometric validation surface area triangle sheet: expected 50, tessellation approximation 50"));
    assert!(result.report().notes.iter().any(|note| note.starts_with(
        "geometric validation centroid triangle centroid: expected (3.333333333333333,3.333333333333333,0), tessellation approximation distance"
    )));
    assert!(result.report().notes.iter().any(
        |note| note == "geometric validation volume open sheet volume: expected 0, tessellation approximation 0"
    ));
    assert!(!result.report().losses.iter().any(|loss| loss
        .message
        .contains("does not match transferred tessellation")));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_tessellated_face_retains_its_surface_carrier() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#7=COMPLEX_TRIANGULATED_FACE('strip and fan',#6,4,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.)),$,(4,3,2,1),((1,2,3,4)),((1,2,4)));",
        "#7=COMPLEX_TRIANGULATED_FACE('strip and fan',#6,4,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.)),#90,(4,3,2,1),((1,2,3,4)),((1,2,4)));",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#90=PLANE('',#34);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode tessellated face surface");

    let surface = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#90")
        .expect("tessellated face surface");
    assert_eq!(
        surface
            .source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("#7")
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_tessellation_partials_transfer_coordinates_and_indices() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#3=COORDINATES_LIST('triangle coordinates',3,((0.,0.,0.),(10.,0.,0.),(0.,10.,0.)));",
        "#3=(COORDINATES_LIST(3,((0.,0.,0.),(10.,0.,0.),(0.,10.,0.))) GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('triangle coordinates') TESSELLATED_ITEM());",
    )
    .replace(
        "#4=TRIANGULATED_FACE('triangle',#3,3,((0.,0.,1.)),$,(),((1,2,3)));",
        "#4=(GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('triangle') TESSELLATED_FACE(#3,3,((0.,0.,1.)),$) TESSELLATED_ITEM() TESSELLATED_STRUCTURED_ITEM() TRIANGULATED_FACE((),((1,2,3))));",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex tessellation partials");

    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.ends_with("#4"))
        .expect("complex tessellated face");
    assert_eq!(mesh.vertices.len(), 3);
    assert_point3_close(mesh.vertices[1], Point3::new(10.0, 0.0, 0.0));
    assert_eq!(mesh.triangles, [[0, 1, 2]]);
    assert_eq!(mesh.normals.len(), 3);
    assert_eq!(
        mesh.body.as_ref().map(cadmpeg_ir::ids::BodyId::as_str),
        Some("step:data:body#38")
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn tessellation_geometry_sets_transfer_flag_and_invalid_pnindex_is_rejected() {
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!(
                "../../../tests/fixtures/ap242_tessellation.p21"
            )),
            &DecodeOptions::default(),
        )
        .expect("decode tessellation fixture");
    assert!(result.report().geometry_transferred);
    assert!(result
        .ir()
        .model
        .tessellations
        .iter()
        .any(|mesh| mesh.id == "step:tessellation:mesh#7" && mesh.body.is_none()));
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemUndeclared.kind()
            && loss.message.contains("mesh retained as detached")
    }));

    let malformed = decode_inline(
        "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,$,('bad'),((1,2,3)));",
    );
    assert!(malformed.ir().model.tessellations.is_empty());
    assert!(malformed
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("invalid pnindex")));
}

#[test]
fn product_linked_bodyless_tessellated_representation_declares_mesh() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#80=PRODUCT_DEFINITION_SHAPE('', '', #81);\n#81=PRODUCT_DEFINITION('', '', '', #82);\n#82=PRODUCT_DEFINITION_FORMATION('', '', #83);\n#83=PRODUCT('', '', '', ());\n#84=SHAPE_DEFINITION_REPRESENTATION(#80,#8);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode product-linked bodyless tessellated representation");
    assert!(decoded
        .ir()
        .model
        .tessellations
        .iter()
        .any(|mesh| mesh.id == "step:tessellation:mesh#7" && mesh.body.is_none()));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemUndeclared.kind()
            && loss.message.contains("tessellation item #7")
    }));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemBodyUnresolved.kind()
            && loss.message.contains("tessellation item #7")
    }));
}

#[test]
fn accuracy_parameter_representation_uses_inherited_items_and_context() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#8=TESSELLATED_SHAPE_REPRESENTATION('complex mesh',(#7),#2);",
        "#8=TESSELLATED_SHAPE_REPRESENTATION_WITH_ACCURACY_PARAMETERS('complex mesh',(#7),#2,(CHORDAL_DEVIATION(0.1)));",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#90=SHAPE_REPRESENTATION_RELATIONSHIP('', '', #39, #8);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode accuracy-parameter tessellated representation");

    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id == "step:tessellation:mesh#7")
        .expect("accuracy-parameter tessellation");
    assert_eq!(
        mesh.body.as_ref().map(cadmpeg_ir::ids::BodyId::as_str),
        Some("step:data:body#38")
    );
    assert!(!decoded.report().losses.iter().any(|loss| {
        (loss.code == StepLossCode::TessellationItemUndeclared.kind()
            || loss.code == StepLossCode::TessellationItemBodyUnresolved.kind())
            && loss.message.contains("tessellation item #7")
    }));
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP native namespace")
        .iter()
        .any(|record| {
            record.id.as_str()
                == "step:data:tessellated_shape_representation_with_accuracy_parameters#8"
        }));
}

#[test]
fn repositioned_annotation_mesh_transfers_one_placement() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#80=CARTESIAN_POINT('',(100.,200.,300.));\n#81=DIRECTION('',(0.,0.,1.));\n#82=DIRECTION('',(1.,0.,0.));\n#83=AXIS2_PLACEMENT_3D('annotation placement',#80,#81,#82);\n#84=(GEOMETRIC_REPRESENTATION_ITEM() REPOSITIONED_TESSELLATED_ITEM(#83) REPRESENTATION_ITEM('repositioned mesh') TESSELLATED_GEOMETRIC_SET((#7)) TESSELLATED_ITEM());\n#85=TESSELLATED_ANNOTATION_OCCURRENCE('repositioned mesh',(),#84);\n#86=(GEOMETRIC_REPRESENTATION_ITEM() REPOSITIONED_TESSELLATED_ITEM(#83) REPRESENTATION_ITEM('repositioned exact mesh') TESSELLATED_GEOMETRIC_SET((#4)) TESSELLATED_ITEM());\n#87=TESSELLATED_ANNOTATION_OCCURRENCE('repositioned exact mesh',(),#86);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode repositioned annotation tessellation");
    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id == "step:tessellation:mesh#7")
        .expect("repositioned annotation mesh");
    assert_point3_close(mesh.vertices[0], Point3::new(110.0, 210.0, 300.0));
    assert_vector3_close(mesh.normals[0], Vector3::new(1.0, 0.0, 0.0));
    assert!(mesh.body.is_none());
    let exact_mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id == "step:tessellation:mesh#4")
        .expect("exact body mesh");
    assert_point3_close(exact_mesh.vertices[0], Point3::new(0.0, 0.0, 0.0));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemUndeclared.kind()
            && loss.message.contains("tessellation item #7")
    }));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemBodyUnresolved.kind()
            && loss.message.contains("tessellation item #7")
    }));
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP native namespace")
        .iter()
        .any(|record| record.id.0.ends_with("#84")));
}

#[test]
fn repositioned_annotation_mesh_rejects_conflicting_placements() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#80=CARTESIAN_POINT('',(100.,200.,300.));\n#81=DIRECTION('',(0.,0.,1.));\n#82=DIRECTION('',(1.,0.,0.));\n#83=AXIS2_PLACEMENT_3D('first placement',#80,#81,#82);\n#84=(GEOMETRIC_REPRESENTATION_ITEM() REPOSITIONED_TESSELLATED_ITEM(#83) REPRESENTATION_ITEM('first repositioned mesh') TESSELLATED_GEOMETRIC_SET((#7)) TESSELLATED_ITEM());\n#85=TESSELLATED_ANNOTATION_OCCURRENCE('first repositioned mesh',(),#84);\n#86=CARTESIAN_POINT('',(-100.,-200.,-300.));\n#87=AXIS2_PLACEMENT_3D('second placement',#86,#81,#82);\n#88=(GEOMETRIC_REPRESENTATION_ITEM() REPOSITIONED_TESSELLATED_ITEM(#87) REPRESENTATION_ITEM('second repositioned mesh') TESSELLATED_GEOMETRIC_SET((#7)) TESSELLATED_ITEM());\n#89=TESSELLATED_ANNOTATION_OCCURRENCE('second repositioned mesh',(),#88);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode conflicting repositioned annotation tessellation");
    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id == "step:tessellation:mesh#7")
        .expect("conflicting repositioned annotation mesh");
    assert_point3_close(mesh.vertices[0], Point3::new(10.0, 10.0, 0.0));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationPlacementAmbiguous.kind()
            && loss.message.contains("tessellation item #7")
    }));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemUndeclared.kind()
            && loss.message.contains("tessellation item #7")
    }));
}

#[test]
fn tessellated_shape_relationship_supplies_exact_body_owner() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#40=TESSELLATED_SHELL('sheet mesh',(#4),#37);",
        "#40=TESSELLATED_SHELL('sheet mesh',(#4),$);",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#90=SHAPE_REPRESENTATION_RELATIONSHIP('','',#39,#5);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode related tessellation");

    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id == "step:tessellation:mesh#4")
        .expect("related mesh");
    assert_eq!(
        mesh.body.as_ref().map(cadmpeg_ir::ids::BodyId::as_str),
        Some("step:data:body#38")
    );
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemBodyUnresolved.kind()
            && loss.message.contains("tessellation item #4")
    }));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DecodeWarning.kind()
            && loss.message.contains("TESSELLATED_SHELL #40")
    }));
}

#[test]
fn direct_tessellated_representation_item_uses_exact_body_relationship() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#90=SHAPE_REPRESENTATION_RELATIONSHIP('', '', #39, #8);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode directly represented tessellation");

    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id == "step:tessellation:mesh#7")
        .expect("directly represented mesh");
    assert_eq!(
        mesh.body.as_ref().map(cadmpeg_ir::ids::BodyId::as_str),
        Some("step:data:body#38")
    );
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemUndeclared.kind()
            && loss.message.contains("tessellation item #7")
    }));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemBodyUnresolved.kind()
            && loss.message.contains("tessellation item #7")
    }));
}

#[test]
fn nested_tessellated_body_container_uses_exact_body_link() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#40=TESSELLATED_SHELL('sheet mesh',(#4),#37);",
        "#40=TESSELLATED_SHELL('sheet mesh',(#91),#37);",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#91=TESSELLATED_GEOMETRIC_SET('nested mesh',(#4));\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode nested body-container tessellation");

    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id == "step:tessellation:mesh#4")
        .expect("nested body-container mesh");
    assert_eq!(
        mesh.body.as_ref().map(cadmpeg_ir::ids::BodyId::as_str),
        Some("step:data:body#38")
    );
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemBodyUnresolved.kind()
            && loss.message.contains("tessellation item #4")
    }));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemUndeclared.kind()
            && loss.message.contains("tessellation item #4")
    }));
}

#[test]
fn nested_tessellated_representation_item_uses_exact_body_relationship() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#8=TESSELLATED_SHAPE_REPRESENTATION('complex mesh',(#7),#2);",
        "#8=TESSELLATED_SHAPE_REPRESENTATION('complex mesh',(#91),#2);",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#91=TESSELLATED_GEOMETRIC_SET('nested mesh',(#7));\n#90=SHAPE_REPRESENTATION_RELATIONSHIP('', '', #39, #8);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode nested represented tessellation");

    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id == "step:tessellation:mesh#7")
        .expect("nested represented mesh");
    assert_eq!(
        mesh.body.as_ref().map(cadmpeg_ir::ids::BodyId::as_str),
        Some("step:data:body#38")
    );
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemUndeclared.kind()
            && loss.message.contains("tessellation item #7")
    }));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemBodyUnresolved.kind()
            && loss.message.contains("tessellation item #7")
    }));
}

#[test]
fn complex_tessellated_shape_representation_inherits_items() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#5=TESSELLATED_SHAPE_REPRESENTATION('mesh',(#40),#2);",
        "#5=(CHARACTERIZED_REPRESENTATION() REPRESENTATION('mesh',(#40),#2) TESSELLATED_SHAPE_REPRESENTATION());",
    )
    .replace(
        "#40=TESSELLATED_SHELL('sheet mesh',(#4),#37);",
        "#40=TESSELLATED_SHELL('sheet mesh',(#4),$);",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#90=SHAPE_REPRESENTATION_RELATIONSHIP('','',#39,#5);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode complex tessellated representation");

    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id == "step:tessellation:mesh#4")
        .expect("complex representation mesh");
    assert_eq!(
        mesh.body.as_ref().map(cadmpeg_ir::ids::BodyId::as_str),
        Some("step:data:body#38")
    );
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemBodyUnresolved.kind()
            && loss.message.contains("tessellation item #4")
    }));
}

#[test]
fn shared_tessellation_item_is_not_assigned_to_an_arbitrary_body() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#80=WIRE_SHELL('',(#32));\n#81=SHELL_BASED_WIREFRAME_MODEL('',(#80));\n#82=TESSELLATED_SHELL('shared mesh',(#4),#80);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode shared tessellation item");
    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id == "step:tessellation:mesh#4")
        .expect("shared mesh");
    assert!(mesh.body.is_none());
    assert!(
        decoded.report().losses.iter().any(|loss| {
            loss.code == StepLossCode::TessellationItemBodyUnresolved.kind()
                && loss.message.contains("multiple candidate bodies")
        }),
        "{:#?}",
        decoded.report().losses
    );
}

#[test]
fn malformed_complex_strip_does_not_discard_valid_strips() {
    let result = decode_inline(
        "#1=COORDINATES_LIST('',4,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.),(1.,1.,0.)));
#2=COMPLEX_TRIANGULATED_SURFACE_SET('',#1,4,$,$,((1,2),(1,2,3,4)),());",
    );
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert_eq!(result.ir().model.tessellations[0].triangles.len(), 2);
}

#[test]
fn complex_triangle_strip_alternates_winding() {
    let result = decode_inline(
        "#1=COORDINATES_LIST('',4,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.),(1.,1.,0.)));
#2=COMPLEX_TRIANGULATED_SURFACE_SET('',#1,4,$,$,((1,2,3,4)),());",
    );

    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert_eq!(
        result.ir().model.tessellations[0].triangles,
        [[0, 1, 2], [2, 1, 3]]
    );
}

#[test]
fn complex_strip_and_malformed_strip_witnesses_preserve_winding() {
    let cases = [
        (
            include_bytes!("tests/data/ap07_complex_strip_and_fan.p21").as_slice(),
            vec![[0, 1, 2], [2, 1, 3], [0, 3, 4]],
        ),
        (
            include_bytes!("tests/data/ap07_malformed_short_strip.p21").as_slice(),
            vec![[0, 1, 2], [2, 1, 3]],
        ),
    ];
    for (input, expected) in cases {
        let result = StepCodec::default()
            .decode(&mut Cursor::new(input), &DecodeOptions::default())
            .expect("decode strip witness");
        assert_eq!(result.ir().model.tessellations.len(), 1);
        assert_eq!(result.ir().model.tessellations[0].triangles, expected);
    }
}

#[test]
fn non_finite_tessellation_coordinates_are_rejected() {
    let result = decode_inline(
        "#1=COORDINATES_LIST('',1,((1E400,0.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,1,$,$,((1,1,1)));",
    );
    assert!(result.ir().model.tessellations.is_empty());
}
#[test]
fn complex_tessellated_face_keeps_exact_support_surface_reachable() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#7=COMPLEX_TRIANGULATED_FACE('strip and fan',#6,4,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.)),$,(4,3,2,1),((1,2,3,4)),((1,2,4)));",
        "#7=COMPLEX_TRIANGULATED_FACE('strip and fan',#6,4,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.)),#79,(4,3,2,1),((1,2,3,4)),((1,2,4)));",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#79=PLANE('exact support',#34);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let result = StepCodec::default()
        .decode(
            &mut Cursor::new(source.as_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode complex tessellated support");
    let support = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.0 == "step:data:surface#79")
        .expect("exact support surface");
    assert_eq!(
        support
            .source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("#7")
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(!validation.findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::Check::CarrierReachability
            && finding.entity.as_deref() == Some("step:data:surface#79")
    }));
}
