// SPDX-License-Identifier: Apache-2.0
//! AP242 indexed tessellation tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use std::fmt::Write as _;
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::{Point3, Vector3};

use crate::loss::StepLossCode;
use crate::parse::Value;
use crate::test_support::exchange::{decode_inline, decode_inline_result};
use crate::StepCodec;

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

fn decode_tessellation_under_policy(
    records: &str,
    policy: DecodePolicy,
) -> Result<CadIr, CodecError> {
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;"
    );
    let (exchange, _) = crate::parse::parse(source.as_bytes()).expect("test exchange parses");
    let mut ir = CadIr::empty();
    let geometry = super::super::geometry::decode(&exchange, &mut ir).value;
    let index = super::super::index::CarrierIndex::from_ir(&ir);
    let topology = super::super::topology::decode(&exchange, &mut ir, &index, None)
        .expect("test topology decodes")
        .value;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)?;
    super::decode(&exchange, &geometry, &topology, &mut ir, &ctx)?;
    Ok(ir)
}

#[test]
fn tessellation_normal_rows_preserve_extreme_finite_directions() {
    let rows = Value::List(vec![
        Value::List(vec![
            Value::Real(f64::MAX),
            Value::Real(0.0),
            Value::Real(0.0),
        ]),
        Value::List(vec![
            Value::Real(2.0_f64.powi(-800)),
            Value::Real(0.0),
            Value::Real(0.0),
        ]),
        Value::List(vec![
            Value::Real(f64::from_bits(1)),
            Value::Real(0.0),
            Value::Real(0.0),
        ]),
    ]);
    assert_eq!(
        super::normal_rows(Some(&rows)),
        Some(vec![
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        ])
    );
}

#[test]
fn tessellation_invalid_normal_rows_are_reported_and_omitted() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.))",
        "((0.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.))",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode invalid normal-row tessellation");
    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7")
        .expect("invalid-normal tessellation");
    assert!(mesh.vertex_normals().is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DecodeWarning.kind()
            && loss
                .message
                .contains("COMPLEX_TRIANGULATED_FACE #7 has invalid normal rows; normals omitted")
    }));
}

#[test]
fn tessellation_empty_normal_rows_mean_unshaded_without_a_warning() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#4=TRIANGULATED_FACE('triangle',#3,3,((0.,0.,1.)),$,(),((1,2,3)));",
        "#4=TRIANGULATED_FACE('triangle',#3,3,(),$,(),((1,2,3)));",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode empty-normal tessellation");
    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#4")
        .expect("empty-normal tessellation");
    assert!(mesh.vertex_normals().is_empty());
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DecodeWarning.kind()
            && loss
                .message
                .contains("TRIANGULATED_FACE #4 has invalid normal rows")
    }));
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
    assert_eq!(mesh.vertices().len(), 3);
    assert!((mesh.vertices()[1].x - 10.0).abs() < EPS_SAME_POINT);
    assert_eq!(mesh.triangles(), [[0, 1, 2]]);
    assert_eq!(mesh.vertex_normals().len(), 3);
    assert_eq!(
        mesh.body.as_ref().map(cadmpeg_ir::ids::BodyId::as_str),
        Some("step:data:body#38")
    );
    let complex = result
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str().ends_with("#7"))
        .unwrap();
    assert_eq!(complex.triangles(), [[0, 1, 2], [2, 1, 3], [0, 1, 3]]);
    assert_point3_close(complex.vertices()[0].get(), Point3::new(10.0, 10.0, 0.0));
    assert_eq!(complex.vertex_normals().len(), 4);
    assert!((complex.vertex_normals()[0].x - 1.0).abs() < EPS_SAME_POINT);
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
        .find(|mesh| mesh.id.as_str().ends_with("#4"))
        .expect("complex tessellated face");
    assert_eq!(mesh.vertices().len(), 3);
    assert_point3_close(mesh.vertices()[1].get(), Point3::new(10.0, 0.0, 0.0));
    assert_eq!(mesh.triangles(), [[0, 1, 2]]);
    assert_eq!(mesh.vertex_normals().len(), 3);
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
    assert!(result.report().geometry_transferred());
    assert!(result
        .ir()
        .model
        .tessellations
        .iter()
        .any(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7" && mesh.body.is_none()));
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
        .any(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7" && mesh.body.is_none()));
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
fn generic_representation_relationship_does_not_admit_product_tessellation() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#39=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#38),#2);",
        "#39=SHAPE_REPRESENTATION('carrier',(#10),#2);",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#80=PRODUCT_DEFINITION_SHAPE('', '', #81);\n#81=PRODUCT_DEFINITION('', '', '', #82);\n#82=PRODUCT_DEFINITION_FORMATION('', '', #83);\n#83=PRODUCT('', '', '', ());\n#84=SHAPE_DEFINITION_REPRESENTATION(#80,#39);\n#90=REPRESENTATION_RELATIONSHIP('generic bridge','',#39,#8);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode generic representation relationship witness");
    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7")
        .expect("generic bridge tessellation");
    assert!(mesh.body.is_none());
    assert_eq!(
        mesh.source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("#7")
    );
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemUndeclared.kind()
            && loss.message.contains("tessellation item #7")
    }));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemBodyUnresolved.kind()
            && loss.message.contains("tessellation item #7")
    }));
}

#[test]
fn shape_representation_relationship_admits_product_tessellation() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "#39=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#38),#2);",
        "#39=SHAPE_REPRESENTATION('carrier',(#10),#2);",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#80=PRODUCT_DEFINITION_SHAPE('', '', #81);\n#81=PRODUCT_DEFINITION('', '', '', #82);\n#82=PRODUCT_DEFINITION_FORMATION('', '', #83);\n#83=PRODUCT('', '', '', ());\n#84=SHAPE_DEFINITION_REPRESENTATION(#80,#39);\n#90=(REPRESENTATION_RELATIONSHIP('typed shape bridge','',#39,#8) SHAPE_REPRESENTATION_RELATIONSHIP());\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode typed shape representation relationship witness");
    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7")
        .expect("typed bridge tessellation");
    assert!(mesh.body.is_none());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemBodyUnresolved.kind()
            && loss.message.contains("tessellation item #7")
    }));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationItemUndeclared.kind()
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
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7")
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
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7")
        .expect("repositioned annotation mesh");
    assert_point3_close(mesh.vertices()[0].get(), Point3::new(110.0, 210.0, 300.0));
    assert_vector3_close(mesh.vertex_normals()[0].get(), Vector3::new(1.0, 0.0, 0.0));
    assert!(mesh.body.is_none());
    let exact_mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#4")
        .expect("exact body mesh");
    assert_point3_close(exact_mesh.vertices()[0].get(), Point3::new(0.0, 0.0, 0.0));
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
        .any(|record| record.id.as_str().ends_with("#84")));
}

#[test]
fn repositioned_annotation_mesh_preserves_extreme_source_normals() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.))",
        "((1.E308,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.))",
    )
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#80=CARTESIAN_POINT('',(100.,200.,300.));\n#81=DIRECTION('',(0.,0.,1.));\n#82=DIRECTION('',(1.,0.,0.));\n#83=AXIS2_PLACEMENT_3D('annotation placement',#80,#81,#82);\n#84=(GEOMETRIC_REPRESENTATION_ITEM() REPOSITIONED_TESSELLATED_ITEM(#83) REPRESENTATION_ITEM('repositioned mesh') TESSELLATED_GEOMETRIC_SET((#7)) TESSELLATED_ITEM());\n#85=TESSELLATED_ANNOTATION_OCCURRENCE('repositioned mesh',(),#84);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode repositioned extreme-normal tessellation");
    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7")
        .expect("repositioned extreme-normal mesh");
    assert_point3_close(mesh.vertices()[0].get(), Point3::new(110.0, 210.0, 300.0));
    assert_vector3_close(mesh.vertex_normals()[0].get(), Vector3::new(1.0, 0.0, 0.0));
    assert!(!decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::DecodeWarning.kind()
            && loss
                .message
                .contains("normal placement could not produce finite unit normals")
    }));
}

#[test]
fn repositioned_annotation_mesh_with_invalid_or_missing_placement_keeps_source_coordinates() {
    for (source, unresolved_placement) in [
        (
            include_bytes!("tests/data/ts01_repositioned_missing_placement.p21").as_slice(),
            false,
        ),
        (
            include_bytes!("tests/data/ts01_repositioned_missing_placement_slot.p21").as_slice(),
            false,
        ),
        (
            include_bytes!("tests/data/ts01_repositioned_unresolved_placement.p21").as_slice(),
            true,
        ),
    ] {
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("decode invalid repositioned placement tessellation");
        let mesh = decoded
            .ir()
            .model
            .tessellations
            .iter()
            .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#4")
            .expect("invalid-placement tessellation");
        assert_point3_close(mesh.vertices()[1].get(), Point3::new(10.0, 0.0, 0.0));
        assert!(decoded.report().losses.iter().any(|loss| {
            loss.code == StepLossCode::TessellationPlacementUnresolved.kind()
                && loss.message.contains("repositioned tessellated item #5")
                && loss.message.contains("unresolved placement is not applied")
        }));
        assert!(decoded
            .ir()
            .native_unknowns("step")
            .expect("STEP native namespace")
            .iter()
            .any(|record| record.id.as_str().ends_with("#5")));
        if unresolved_placement {
            assert!(decoded
                .ir()
                .native_unknowns("step")
                .expect("STEP native namespace")
                .iter()
                .any(|record| record.id.as_str() == "step:data:axis2_placement_3d#99"));
        }
        let validation =
            cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn unresolved_outer_repositioning_preserves_inner_valid_placement() {
    let source = String::from_utf8(
        include_bytes!("../../../tests/fixtures/ap242_tessellation.p21").to_vec(),
    )
    .expect("fixture is UTF-8")
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#80=CARTESIAN_POINT('',(100.,200.,300.));\n#81=DIRECTION('',(0.,0.,1.));\n#82=DIRECTION('',(1.,0.,0.));\n#83=AXIS2_PLACEMENT_3D('inner placement',#80,#81,#82);\n#84=(GEOMETRIC_REPRESENTATION_ITEM() REPOSITIONED_TESSELLATED_ITEM(#83) REPRESENTATION_ITEM('inner mesh') TESSELLATED_GEOMETRIC_SET((#7)) TESSELLATED_ITEM());\n#85=TESSELLATED_ANNOTATION_OCCURRENCE('inner mesh',(),#84);\n#88=(GEOMETRIC_REPRESENTATION_ITEM() REPOSITIONED_TESSELLATED_ITEM(#85) REPRESENTATION_ITEM('unresolved outer placement') TESSELLATED_GEOMETRIC_SET((#84)) TESSELLATED_ITEM());\n#89=TESSELLATED_ANNOTATION_OCCURRENCE('unresolved outer placement',(),#88);\nENDSEC;\nEND-ISO-10303-21;",
    );
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode nested unresolved repositioning");
    let mesh = decoded
        .ir()
        .model
        .tessellations
        .iter()
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7")
        .expect("nested repositioned annotation mesh");
    assert_point3_close(mesh.vertices()[0].get(), Point3::new(110.0, 210.0, 300.0));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == StepLossCode::TessellationPlacementUnresolved.kind()
            && loss.message.contains("repositioned tessellated item #88")
            && loss.message.contains("unresolved placement is not applied")
    }));
    assert!(decoded
        .ir()
        .native_unknowns("step")
        .expect("STEP native namespace")
        .iter()
        .any(|record| record.id.as_str().ends_with("#88")));
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
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7")
        .expect("conflicting repositioned annotation mesh");
    assert_point3_close(mesh.vertices()[0].get(), Point3::new(10.0, 10.0, 0.0));
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
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#4")
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
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7")
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
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#4")
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
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7")
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
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#4")
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
        .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#4")
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
fn malformed_complex_strip_refuses_the_aggregate() {
    let error = decode_inline_result(
        "#1=COORDINATES_LIST('',4,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.),(1.,1.,0.)));
#2=COMPLEX_TRIANGULATED_SURFACE_SET('',#1,4,$,$,((1,2),(1,2,3,4)),());",
    )
    .expect_err("a short strip must refuse the aggregate");
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(message)) if message.contains("#2 strip row 1"))
    );
}

#[test]
fn tri_ext1_2_nonreference_tessellation_item_refuses_the_aggregate() {
    let error = decode_inline_result(
        "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,$,$,((1,2,3)));
#3=TESSELLATED_SOLID('',(#2,$),$);",
    )
    .expect_err("a non-reference item must refuse the aggregate");
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(message)) if message.contains("TESSELLATED_SOLID #3 item 2"))
    );
}

#[test]
fn malformed_complex_fan_refuses_the_aggregate() {
    let error = decode_inline_result(
        "#1=COORDINATES_LIST('',4,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.),(1.,1.,0.)));
#2=COMPLEX_TRIANGULATED_SURFACE_SET('',#1,4,$,$,(),((1,2),(1,2,3,4)));",
    )
    .expect_err("a short fan must refuse the aggregate");
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(CodecError::Malformed(message)) if message.contains("#2 fan row 1"))
    );
}

#[test]
fn nested_tessellation_container_nonreference_item_is_refused() {
    let error = decode_inline_result(
        "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,$,$,((1,2,3)));
#3=TESSELLATED_GEOMETRIC_SET('',(#2,$));
#4=TESSELLATED_SOLID('',(#3),$);",
    )
    .expect_err("nested non-reference item must refuse the aggregate");
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(CodecError::Malformed(message)) if message.contains("TESSELLATED_GEOMETRIC_SET #3 item 2"))
    );
}

#[test]
fn tessellation_association_depth_uses_the_session_limit() {
    let mut records = String::from(
        "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));\n#2=TRIANGULATED_SURFACE_SET('',#1,3,$,$,((1,2,3)));\n",
    );
    for id in 3..=15 {
        writeln!(
            records,
            "#{id}=TESSELLATED_GEOMETRIC_SET('',(#{}));",
            id - 1
        )
        .expect("write nested set");
    }
    records.push_str("#16=TESSELLATED_SOLID('',(#15),$);");
    let service = DecodePolicy::service();
    let accepted = decode_tessellation_under_policy(&records, service)
        .expect("service depth admits the association chain");
    assert_eq!(accepted.model.tessellations.len(), 1);
    let mut limited = service;
    limited.limits.max_recursion_depth = 13;
    let error = decode_tessellation_under_policy(&records, limited)
        .expect_err("association chain exceeds the selected depth");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RecursionDepth && limit.operation == "step_tessellation_association")
    );
}

#[test]
fn tessellation_representation_body_walk_uses_the_session_limit() {
    let mut records = String::from("#1=TESSELLATED_SHAPE_REPRESENTATION('mesh',(),$);\n");
    for id in 2..=15 {
        writeln!(records, "#{id}=SHAPE_REPRESENTATION('',(),$);").expect("write representation");
        writeln!(
            records,
            "#{}=SHAPE_REPRESENTATION_RELATIONSHIP('','',#{},#{id});",
            id + 100,
            id - 1
        )
        .expect("write relationship");
    }
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(&records, service)
        .expect("service depth admits the representation chain");
    let mut limited = service;
    limited.limits.max_recursion_depth = 13;
    let error = decode_tessellation_under_policy(&records, limited)
        .expect_err("representation chain exceeds the selected depth");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RecursionDepth && limit.operation == "step_representation_body_walk")
    );
}

#[test]
fn single_tessellation_normal_replication_charges_collection_items() {
    let records = "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,((0.,0.,1.)),$,((1,2,3)));";
    let service = DecodePolicy::service();
    let accepted = decode_tessellation_under_policy(records, service)
        .expect("service items admit three replicated normals");
    assert_eq!(accepted.model.tessellations[0].vertex_normals().len(), 3);
    let mut limited = service;
    limited.limits.max_collection_items = 2;
    let error = decode_tessellation_under_policy(records, limited)
        .expect_err("three normal copies exceed the selected item limit");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_tessellation_normal_replication")
    );
}

#[test]
fn tessellation_container_items_charge_before_collection() {
    let records = "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,$,$,((1,2,3)));
#3=TESSELLATED_SOLID('',(#2,#2),$);";
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(records, service).expect("service admits both items");
    let mut limited = service;
    limited.limits.max_collection_items = 1;
    let error = decode_tessellation_under_policy(records, limited)
        .expect_err("two container items exceed one admitted item");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_tessellation_container_items")
    );
}

#[test]
fn complex_tessellation_rows_charge_before_collection() {
    let records = "#1=COORDINATES_LIST('',4,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.),(1.,1.,0.)));
#2=COMPLEX_TRIANGULATED_SURFACE_SET('',#1,4,$,$,((1,2,3),(1,2,3,4)),());";
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(records, service).expect("service admits both strips");
    let mut limited = service;
    limited.limits.max_collection_items = 1;
    let error = decode_tessellation_under_policy(records, limited)
        .expect_err("two strip rows exceed one admitted item");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_complex_tessellation_rows")
    );
}

#[test]
fn complex_tessellation_indices_charge_before_collection() {
    let records = "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=COMPLEX_TRIANGULATED_SURFACE_SET('',#1,3,$,$,((1,2,3)),());";
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(records, service).expect("service admits three indices");
    let mut limited = service;
    limited.limits.max_collection_items = 3;
    let error = decode_tessellation_under_policy(records, limited)
        .expect_err("one row plus three indices exceed three admitted items");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_complex_tessellation_indices")
    );
}

#[test]
fn complex_tessellation_triangles_charge_before_collection() {
    let records = "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=COMPLEX_TRIANGULATED_SURFACE_SET('',#1,3,$,$,((1,2,3)),());";
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(records, service).expect("service admits one triangle");
    let mut limited = service;
    limited.limits.max_collection_items = 4;
    let error = decode_tessellation_under_policy(records, limited)
        .expect_err("one triangle exceeds the four prior admitted items");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "step_complex_tessellation_triangles")
    );
}

#[test]
fn complex_triangle_strip_alternates_winding() {
    let result = decode_inline(
        "#1=COORDINATES_LIST('',4,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.),(1.,1.,0.)));
#2=COMPLEX_TRIANGULATED_SURFACE_SET('',#1,4,$,$,((1,2,3,4)),());",
    );

    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert_eq!(
        result.ir().model.tessellations[0].triangles(),
        [[0, 1, 2], [2, 1, 3]]
    );
}

#[test]
fn complex_strip_and_malformed_strip_witnesses_preserve_winding() {
    let valid = include_bytes!("tests/data/ap07_complex_strip_and_fan.p21").as_slice();
    let result = StepCodec::default()
        .decode(&mut Cursor::new(valid), &DecodeOptions::default())
        .expect("decode strip and fan witness");
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert_eq!(
        result.ir().model.tessellations[0].triangles(),
        [[0, 1, 2], [2, 1, 3], [0, 3, 4]]
    );
    let malformed = include_bytes!("tests/data/ap07_malformed_short_strip.p21").as_slice();
    let error = StepCodec::default()
        .decode(&mut Cursor::new(malformed), &DecodeOptions::default())
        .expect_err("short strip must refuse the aggregate");
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(message)) if message.contains("#2 strip row 1"))
    );
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
        .find(|surface| surface.id.as_str() == "step:data:surface#79")
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
        finding.check == cadmpeg_ir::report::check::Check::CarrierReachability
            && finding.entity.as_deref() == Some("step:data:surface#79")
    }));
}
