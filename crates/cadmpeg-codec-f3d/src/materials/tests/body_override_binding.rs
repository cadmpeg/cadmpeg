// SPDX-License-Identifier: Apache-2.0

use crate::materials::bind_body_override;
use cadmpeg_ir::appearance::{AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::codec::Codec;
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
        precedence: None,
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

    let mut losses = Vec::new();
    bind_body_override(
        &mut bindings,
        &mut Default::default(),
        &mut losses,
        over.clone(),
    );

    let on_first: Vec<_> = bindings
        .iter()
        .filter(|b| b.target.entity == AppearanceTarget::Body(first.clone()))
        .collect();
    assert_eq!(on_first, vec![&over]);
    assert_eq!(on_first[0].appearance, over.appearance);
    assert!(bindings.contains(&other));
    assert_eq!(bindings.len(), 2);
    assert_eq!(losses.len(), 1);
}

#[test]
fn body_override_naming_the_assigned_appearance_keeps_the_assignment_binding() {
    let first = body("1");
    let mut assigned = binding("assigned-1", &first, "material");
    assigned.source_entity_id = Some("0_985".to_owned());
    let over = binding("override-1", &first, "material");
    let mut bindings = vec![assigned.clone()];

    bind_body_override(
        &mut bindings,
        &mut Default::default(),
        &mut Vec::new(),
        over,
    );

    assert_eq!(bindings, vec![assigned]);
}

/// A body-override record repeating the assignment's appearance is examined
/// before the real override when it sorts first; it must not consume the
/// body's override, so the real override still binds and is charged.
#[test]
fn a_repeat_of_the_assigned_appearance_does_not_consume_the_override() {
    let first = body("1");
    let assigned = binding("assigned-1", &first, "material");
    let repeat = binding("override-0", &first, "material");
    let over = binding("override-1", &first, "override");
    let mut bindings = vec![assigned];
    let mut overridden = Default::default();
    let mut losses = Vec::new();

    bind_body_override(&mut bindings, &mut overridden, &mut losses, repeat);
    bind_body_override(&mut bindings, &mut overridden, &mut losses, over.clone());

    assert_eq!(bindings, vec![over]);
    assert_eq!(losses.len(), 1);
}

#[test]
fn the_first_body_override_wins_over_a_later_one() {
    let first = body("1");
    let assigned = binding("assigned-1", &first, "material");
    let earlier = binding("override-1", &first, "red");
    let later = binding("override-2", &first, "blue");
    let mut bindings = vec![assigned];
    let mut overridden = Default::default();

    bind_body_override(
        &mut bindings,
        &mut overridden,
        &mut Vec::new(),
        earlier.clone(),
    );
    let mut losses = Vec::new();
    bind_body_override(&mut bindings, &mut overridden, &mut losses, later);

    assert_eq!(bindings, vec![earlier]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::BodyOverrideSuperseded.kind()
    );
}

#[test]
fn decode_binds_the_override_appearance_over_a_different_assignment_and_charges_the_assumption() {
    assert_override_binds(
        "11111111-2222-3333-4444-555555555555",
        "00000000-2222-3333-4444-555555555555",
    );
}

fn assert_override_binds(assigned: &str, overriding: &str) {
    let f3d =
        crate::test_support::zip_fixture::f3d_with_smbh_and_protein_guids_and_browser_override(
            &crate::test_support::smbh_geometry_fixture::synthetic_geometry_smbh(),
            &[assigned, overriding],
            overriding,
        );
    let result = crate::F3dCodec
        .decode(
            &mut std::io::Cursor::new(f3d),
            &cadmpeg_ir::codec::DecodeOptions::default(),
        )
        .expect("override decode");
    let model = &result.ir().model;
    let appearance = |guid: &str| {
        model
            .appearances
            .iter()
            .find(|appearance| appearance.visual_guid.as_deref() == Some(guid))
            .unwrap_or_else(|| panic!("appearance {guid} decoded"))
            .id
            .clone()
    };
    assert_eq!(model.appearance_bindings.len(), 1);
    assert_eq!(
        model.appearance_bindings[0].appearance,
        appearance(overriding)
    );
    assert_ne!(
        model.appearance_bindings[0].appearance,
        appearance(assigned)
    );
    let charged = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == crate::loss::F3dLossCode::BodyOverridePrecedenceAssumed.kind())
        .count();
    assert_eq!(charged, 1);
}
