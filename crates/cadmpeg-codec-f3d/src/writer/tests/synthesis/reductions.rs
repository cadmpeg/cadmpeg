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

