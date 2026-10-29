// SPDX-License-Identifier: Apache-2.0
//! A standalone ACT root has a neutral product label and body membership.

use super::{decode, f3d_native, f3d_with_smbh_and_protein, synthetic_geometry_smbh};
use crate::test_support::native_test::TestEncode;
use crate::F3dCodec;

#[test]
fn single_component_act_root_projects_its_label_and_writes_neutral_edits() {
    let mut decoded = decode(f3d_with_smbh_and_protein(&synthetic_geometry_smbh()));
    let native = f3d_native(decoded.ir());
    let [root] = native.act_component_links.as_slice() else {
        panic!("one ACT root in the generated standalone design");
    };
    let [product] = decoded.ir().model.product_definitions.as_slice() else {
        panic!("one neutral product definition for the document root");
    };
    assert_eq!(product.native_ref.as_deref(), Some(root.id().as_str()));
    assert_eq!(product.label.as_deref(), Some(root.layout().display_name()));
    assert_eq!(
        product.bodies,
        decoded
            .ir()
            .model
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect::<Vec<_>>()
    );
    decoded.ir_mut().model.product_definitions[0].label = Some("Edited root label".into());
    let mut bytes = Vec::new();
    F3dCodec
        .encode(decoded.ir(), &mut bytes)
        .expect("source-free ACT root synthesis");
    let output = decode(bytes);
    let [product] = output.ir().model.product_definitions.as_slice() else {
        panic!("one product remains after native root regeneration");
    };
    assert_eq!(product.label.as_deref(), Some("Edited root label"));
    assert_eq!(product.bodies.len(), output.ir().model.bodies.len());
    assert_eq!(
        output.ir().model.bodies.len(),
        decoded.ir().model.bodies.len()
    );
    let native = f3d_native(output.ir());
    assert_eq!(
        native.act_component_links[0].layout().display_name(),
        "Edited root label"
    );
}
