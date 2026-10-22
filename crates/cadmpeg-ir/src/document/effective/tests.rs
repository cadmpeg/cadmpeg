// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;
use crate::appearance::{Appearance, AppearanceBinding};
use crate::products::{Occurrence, PrototypeReference};
use crate::transform::Transform;

fn binding(target: AppearanceTarget) -> AppearanceBinding {
    AppearanceBinding {
        native_ref: None,
        id: "test:display:binding#one".try_into().unwrap(),
        target,
        appearance: "test:display:appearance#one".try_into().unwrap(),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::default(),
    }
}

fn layer(
    id: &str,
    parent: Option<&str>,
    visible: Option<bool>,
    items: Vec<PresentationItem>,
) -> PresentationLayer {
    PresentationLayer {
        native_ref: None,
        id: id.try_into().unwrap(),
        name: id.into(),
        parent: parent.map(|id| id.try_into().unwrap()),
        description: None,
        visible,
        items,
    }
}

fn occurrence(id: &str, parent: OccurrenceParent, visible: Option<bool>) -> Occurrence {
    Occurrence {
        id: id.try_into().unwrap(),
        prototype: PrototypeReference::Unresolved {},
        parent,
        ordinal: 0,
        transform: Some(Transform::identity()),
        linked_prototype: None,
        name: None,
        visible,
        link: None,
        native_ref: None,
    }
}

#[test]
fn display_colour_wins_without_modifying_the_independent_material() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let target = AppearanceTarget::Body(ir.model.bodies[0].id.clone());
    let material = Color::from_rgba8(20, 40, 60, 255);
    let display = Color::from_rgba8(200, 100, 50, 255);
    ir.model.appearances.push(Appearance {
        native_ref: None,
        id: "test:display:appearance#one".try_into().unwrap(),
        name: None,
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(material),
        properties: std::collections::BTreeMap::default(),
        textures: vec![],
    });
    ir.model.appearance_bindings.push(binding(target.clone()));
    ir.model.bodies[0].color = Some(display);
    assert_eq!(ir.effective_display_color(&target), Some(display));
    assert_eq!(ir.model.appearances[0].base_color, Some(material));
    ir.model.bodies[0].color = None;
    assert_eq!(ir.effective_display_color(&target), Some(material));
}

#[test]
fn hidden_parent_layer_is_a_gate_and_visibility_facts_remain_independent() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let body = ir.model.bodies[0].id.clone();
    let target = AppearanceTarget::Body(body.clone());
    ir.model.bodies[0].visible = Some(true);
    let mut assigned = binding(target.clone());
    assigned.visible = Some(true);
    ir.model.appearance_bindings.push(assigned);
    ir.model.presentation_layers = vec![
        layer("test:display:layer#parent", None, Some(false), vec![]),
        layer(
            "test:display:layer#child",
            Some("test:display:layer#parent"),
            Some(true),
            vec![PresentationItem::Body { body }],
        ),
    ];
    assert_eq!(ir.effective_visibility(&target), Some(false));
    assert_eq!(ir.model.bodies[0].visible, Some(true));
    ir.model.presentation_layers[0].visible = Some(true);
    assert_eq!(ir.effective_visibility(&target), Some(true));
    ir.model.appearance_bindings[0].visible = Some(false);
    assert_eq!(ir.effective_visibility(&target), Some(false));
}

#[test]
fn occurrence_parent_visibility_does_not_hide_independent_instances_or_body_queries() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let body = AppearanceTarget::Body(ir.model.bodies[0].id.clone());
    ir.model.bodies[0].visible = Some(true);
    let parent = occurrence(
        "test:display:occurrence#parent",
        OccurrenceParent::Root {},
        Some(false),
    );
    let child = occurrence(
        "test:display:occurrence#child",
        OccurrenceParent::Occurrence {
            occurrence: parent.id.clone(),
        },
        Some(true),
    );
    let other = occurrence(
        "test:display:occurrence#other",
        OccurrenceParent::Root {},
        Some(true),
    );
    let child_target = AppearanceTarget::Occurrence(child.id.clone());
    let other_target = AppearanceTarget::Occurrence(other.id.clone());
    ir.model.occurrences = vec![parent, child, other];
    assert_eq!(ir.effective_visibility(&child_target), Some(false));
    assert_eq!(ir.effective_visibility(&other_target), Some(true));
    assert_eq!(ir.effective_visibility(&body), Some(true));
}

#[test]
fn absence_of_visibility_is_distinct_from_an_explicit_visible_gate() {
    let ir = CadIr::empty();
    assert_eq!(
        ir.effective_visibility(&AppearanceTarget::Source {
            source_id: "unassigned".into()
        }),
        None
    );
}

#[test]
fn only_the_objects_provider_contributes_view_visibility() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let body = ir.model.bodies[0].id.clone();
    ir.model.bodies[0].visible = None;
    ir.model.view_presentations.push(
        serde_json::from_value(serde_json::json!({
            "id": "test:display:view#named-camera", "order": 0, "visible": false
        }))
        .unwrap(),
    );
    assert_eq!(
        ir.effective_visibility(&AppearanceTarget::Body(body.clone())),
        None
    );
    ir.model.view_presentations.push(
        serde_json::from_value(serde_json::json!({
            "id": "test:display:view#body", "order": 1, "object": body.as_str(), "visible": true
        }))
        .unwrap(),
    );
    assert_eq!(
        ir.effective_visibility(&AppearanceTarget::Body(body.clone())),
        Some(true)
    );
    ir.model.view_presentations[1].visible = Some(false);
    assert_eq!(
        ir.effective_visibility(&AppearanceTarget::Body(body)),
        Some(false)
    );
}
