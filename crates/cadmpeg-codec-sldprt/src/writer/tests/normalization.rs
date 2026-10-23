// SPDX-License-Identifier: Apache-2.0
//! Semantic-writer normalization and neutral admission.

use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::ids::{AppearanceBindingId, AppearanceId};
use std::collections::BTreeMap;

#[test]
fn normalized_appearance_bindings_remain_admissible_when_target_order_differs() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("valid cube");
    let appearance = AppearanceId::mint("sldprt:test:appearance#shared").unwrap();
    ir.model.appearances.push(Appearance {
        id: appearance.clone(),
        name: None,
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: None,
        properties: BTreeMap::new(),
        textures: Vec::new(),
    });
    ir.model.appearance_bindings = vec![
        AppearanceBinding {
            id: AppearanceBindingId::mint("sldprt:test:binding#a-face").unwrap(),
            target: AppearanceTarget::Face(ir.model.faces[0].id.clone()),
            appearance: appearance.clone(),
            source_entity_id: None,
            object_type: None,
            visible: Some(true),
            channels: BTreeMap::new(),
        },
        AppearanceBinding {
            id: AppearanceBindingId::mint("sldprt:test:binding#b-body").unwrap(),
            target: AppearanceTarget::Body(ir.model.bodies[0].id.clone()),
            appearance,
            source_entity_id: None,
            object_type: None,
            visible: Some(false),
            channels: BTreeMap::new(),
        },
    ];
    let before = ir.model.appearance_bindings.clone();
    assert!(cadmpeg_ir::validate_neutral(&ir, Vec::new())
        .unwrap()
        .is_ok());

    crate::writer::sort_arenas(&mut ir);

    assert!(cadmpeg_ir::validate_neutral(&ir, Vec::new())
        .unwrap()
        .is_ok());
    assert_eq!(ir.model.appearance_bindings, before);
}
