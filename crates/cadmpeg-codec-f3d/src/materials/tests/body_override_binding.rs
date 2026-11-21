// SPDX-License-Identifier: Apache-2.0

use crate::materials::bind_body_override;
use cadmpeg_ir::appearance::{AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::ids::{AppearanceId, BodyId};

fn body(name: &str) -> BodyId {
    BodyId::mint(format!("f3d:brep/a.smbh/brep:entity#{name}")).expect("identity grammar")
}

fn binding(id: &str, body: &BodyId, appearance: &str) -> AppearanceBinding {
    AppearanceBinding {
        id: format!("f3d:appearance:body#{id}")
            .try_into()
            .expect("valid identity"),
        target: AppearanceTarget::Body(body.clone()).into(),
        appearance: AppearanceId::mint(format!("f3d:test:appearance#{appearance}"))
            .expect("identity grammar"),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: Default::default(),
        native_ref: None,
    }
}

#[test]
fn body_override_replaces_the_material_assignment_binding_of_that_body_only() {
    let (first, second) = (body("1"), body("2"));
    let assigned = binding("assigned-1", &first, "material");
    let other = binding("assigned-2", &second, "other");
    let over = binding("override-1", &first, "override");
    let mut bindings = vec![assigned, other.clone()];

    bind_body_override(&mut bindings, over.clone());

    let on_first: Vec<_> = bindings
        .iter()
        .filter(|b| b.target.entity == AppearanceTarget::Body(first.clone()))
        .collect();
    assert_eq!(on_first, vec![&over]);
    assert_eq!(on_first[0].appearance, over.appearance);
    assert!(bindings.contains(&other));
    assert_eq!(bindings.len(), 2);
}

#[test]
fn body_override_naming_the_assigned_appearance_keeps_the_assignment_binding() {
    let first = body("1");
    let mut assigned = binding("assigned-1", &first, "material");
    assigned.source_entity_id = Some("0_985".to_owned());
    let over = binding("override-1", &first, "material");
    let mut bindings = vec![assigned.clone()];

    bind_body_override(&mut bindings, over);

    assert_eq!(bindings, vec![assigned]);
}
