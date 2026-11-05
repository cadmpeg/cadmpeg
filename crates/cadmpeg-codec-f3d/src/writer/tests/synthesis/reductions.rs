// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn dangling_coedge_is_reduced_with_a_named_loss() {
    use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
    use crate::test_support::zip_test::f3d_with_smbh;
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec.decode(&mut Cursor::new(source), &DecodeOptions::default()).unwrap();
    let mut ir = decoded.ir().clone();
    let coedge = ir.model.coedges[0].id.clone();
    ir.model.coedges[0].edge = "f3d:brep:entity#missing".try_into().unwrap();
    let plan = F3dCodec.plan(EncodeInput::new(&ir, None), TargetRequest::Inherit).unwrap();
    assert!(plan.report().losses.iter().any(|loss| loss.message.contains(coedge.as_str())));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    F3dCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()).unwrap();
}

#[test]
fn duplicate_attribute_identity_is_named_in_loss() {
    use crate::test_support::smbh_geometry_test::synthetic_geometry_with_attribute_smbh;
    use crate::test_support::zip_test::f3d_with_smbh;
    let decoded = F3dCodec.decode(
        &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_attribute_smbh())),
        &DecodeOptions::default(),
    ).unwrap();
    let mut ir = decoded.ir().clone();
    ir.model.attributes[1].id = ir.model.attributes[0].id.clone();
    let plan = F3dCodec.plan(EncodeInput::new(&ir, None), TargetRequest::Inherit).unwrap();
    assert!(plan.report().losses.iter().any(|loss| {
        loss.message.contains(ir.model.attributes[0].id.as_str())
            && loss.message.contains("duplicate identity")
    }));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = F3dCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()).unwrap();
    assert_eq!(output.ir().model.bodies.len(), 1);
}


#[test]
fn mesh_surface_sentinel_survives_source_less_synthesis() {
    use crate::test_support::smbh_geometry_test::synthetic_geometry_with_mesh_surface_smbh;
    use crate::test_support::zip_test::f3d_with_smbh;
    let source = f3d_with_smbh(&synthetic_geometry_with_mesh_surface_smbh());
    let decoded = F3dCodec.decode(&mut Cursor::new(source), &DecodeOptions::default()).unwrap();
    let original = crate::test_support::native_test::f3d_native(decoded.ir());
    assert_eq!(original.mesh_surface_sentinels.len(), 1);
    let mut bytes = Vec::new();
    let report = F3dCodec.plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
        .unwrap().write_to(&mut bytes).unwrap();
    let output = F3dCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()).unwrap();
    assert_eq!(output.ir().model.faces.len(), decoded.ir().model.faces.len());
    assert_eq!(crate::test_support::native_test::f3d_native(output.ir()).mesh_surface_sentinels,
        original.mesh_surface_sentinels);
    assert!(!report.losses.iter().any(|loss| loss.message.contains("mesh-surface-sentinel")));

    let plane = f3d_with_smbh(&crate::test_support::smbh_geometry_test::synthetic_geometry_smbh());
    let plane = F3dCodec.decode(&mut Cursor::new(plane), &DecodeOptions::default()).unwrap();
    let mut edited = decoded.ir().clone();
    edited.model.surfaces[0].geometry = plane.ir().model.surfaces[0].geometry.clone();
    let mut bytes = Vec::new();
    let report = F3dCodec.plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)
        .unwrap().write_to(&mut bytes).unwrap();
    let output = F3dCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()).unwrap();
    assert!(crate::test_support::native_test::f3d_native(output.ir()).mesh_surface_sentinels.is_empty());
    assert!(report.losses.iter().any(|loss| loss.message.contains("mesh-surface-sentinel")));
}

#[test]
fn source_less_compact_body_attribute_is_authored() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    let source = crate::test_support::zip_test::f3d_with_smbh(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh());
    let decoded = F3dCodec.decode(&mut Cursor::new(source), &DecodeOptions::default()).unwrap();
    let mut ir = decoded.ir().clone();
    let target = AttributeTarget::Body(ir.model.bodies[0].id.clone());
    let values = vec![Some(AttributeValue::Reference("f3d:brep:entity#-1".into())),
        Some(AttributeValue::String("authored-value".into())), Some(AttributeValue::Integer(17))];
    ir.model.attributes.push(SourceAttribute {
        id: "authored:model:attribute#1".try_into().unwrap(), native_ref: None,
        target: target.clone(), name: "ATTRIB_CUSTOM-attrib".into(), values: values.clone(),
    });
    let mut bytes = Vec::new();
    let report = F3dCodec.plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap().write_to(&mut bytes).unwrap();
    let output = F3dCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()).unwrap();
    let attribute = output.ir().model.attributes.iter().find(|attribute| attribute.target == target).unwrap();
    assert_eq!(attribute.name, "ATTRIB_CUSTOM-attrib");
    assert_eq!(attribute.values, values);
    assert!(!report.losses.iter().any(|loss| loss.message.contains("authored:model:attribute#1")));

    ir.model.attributes[0].values[1] = Some(AttributeValue::String("x".repeat(256)));
    let mut bytes = Vec::new();
    let report = F3dCodec.plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap().write_to(&mut bytes).unwrap();
    assert!(report.losses.iter().any(|loss| loss.message.contains("authored:model:attribute#1")));
    let output = F3dCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()).unwrap();
    assert_eq!(output.ir().model.bodies.len(), 1);
}

#[test]
fn source_less_compact_face_attribute_is_authored() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    let source = crate::test_support::zip_test::f3d_with_smbh(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh());
    let decoded = F3dCodec.decode(&mut Cursor::new(source), &DecodeOptions::default()).unwrap();
    let mut ir = decoded.ir().clone();
    let target = AttributeTarget::Face(ir.model.faces[0].id.clone());
    let values = vec![Some(AttributeValue::Reference("f3d:brep:entity#-1".into())),
        Some(AttributeValue::String("face-value".into())), Some(AttributeValue::Integer(42))];
    ir.model.attributes.push(SourceAttribute {
        id: "authored:model:attribute#face".try_into().unwrap(), native_ref: None,
        target: target.clone(), name: "ATTRIB_CUSTOM-attrib".into(), values: values.clone(),
    });
    let body_target = AttributeTarget::Body(ir.model.bodies[0].id.clone());
    ir.model.attributes.push(SourceAttribute {
        id: "authored:model:attribute#body".try_into().unwrap(), native_ref: None,
        target: body_target.clone(), name: "ATTRIB_CUSTOM-attrib".into(), values: values.clone(),
    });
    let mut bytes = Vec::new();
    let report = F3dCodec.plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap().write_to(&mut bytes).unwrap();
    let output = F3dCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()).unwrap();
    let attribute = output.ir().model.attributes.iter().find(|attribute| attribute.target == target).unwrap();
    assert_eq!(attribute.values, values);
    let body_attribute = output.ir().model.attributes.iter().find(|attribute| attribute.target == body_target).unwrap();
    assert_eq!(body_attribute.values, values);
    assert!(!report.losses.iter().any(|loss| loss.message.contains("authored:model:attribute#face")
        || loss.message.contains("authored:model:attribute#body")));
}

#[test]
fn source_less_compact_edge_vertex_and_coedge_attributes_are_authored() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    let source = crate::test_support::zip_test::f3d_with_smbh(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh());
    let decoded = F3dCodec.decode(&mut Cursor::new(source), &DecodeOptions::default()).unwrap();
    let mut ir = decoded.ir().clone();
    let targets = [AttributeTarget::Edge(ir.model.edges[0].id.clone()),
        AttributeTarget::Vertex(ir.model.vertices[0].id.clone()),
        AttributeTarget::Coedge(ir.model.coedges[0].id.clone())];
    for (ordinal, target) in targets.iter().enumerate() {
        ir.model.attributes.push(SourceAttribute {
            id: format!("authored:model:attribute#{ordinal}").try_into().unwrap(), native_ref: None,
            target: target.clone(), name: "ATTRIB_CUSTOM-attrib".into(),
            values: vec![Some(AttributeValue::Reference("f3d:brep:entity#-1".into())),
                Some(AttributeValue::String(format!("value-{ordinal}")))],
        });
    }
    let mut bytes = Vec::new();
    let report = F3dCodec.plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap().write_to(&mut bytes).unwrap();
    let output = F3dCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()).unwrap();
    for (ordinal, target) in targets.iter().enumerate() {
        let attribute = output.ir().model.attributes.iter().find(|attribute| &attribute.target == target).unwrap();
        assert_eq!(attribute.values[1], Some(AttributeValue::String(format!("value-{ordinal}"))));
        assert!(!report.losses.iter().any(|loss| loss.message.contains(&format!("authored:model:attribute#{ordinal}"))));
    }
}

#[test]
fn source_body_without_asm_key_keeps_the_absent_key() {
    let source = crate::test_support::zip_test::f3d_with_smbh(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh());
    let decoded = F3dCodec.decode(&mut Cursor::new(source), &DecodeOptions::default()).unwrap();
    let mut ir = decoded.ir().clone();
    crate::test_support::native_test::f3d_native_mut(&mut ir).body_native_keys[0].asm_body_key = None;
    let before = crate::test_support::native_test::f3d_native(&ir).body_native_keys;
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].asm_body_key, None);
    let mut bytes = Vec::new();
    F3dCodec.plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap().write_to(&mut bytes).unwrap();
    let output = F3dCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()).unwrap();
    assert_eq!(crate::test_support::native_test::f3d_native(output.ir()).body_native_keys, before);
}
