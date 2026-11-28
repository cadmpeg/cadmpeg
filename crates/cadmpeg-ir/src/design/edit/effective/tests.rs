// SPDX-License-Identifier: Apache-2.0
//! The display resolvers on small assemblies.

use cadmpeg_core::serde_erase::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;

use crate::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use crate::design::configuration::{
    Arrangement, ArrangementKind, ArrangementRef, DesignConfiguration, NestedArrangement,
    OccurrenceOverride, Overrides,
};
use crate::design::evaluation::{CurrentState, EvaluationStatus};
use crate::design::refs::ConfigurationRef;
use crate::design::refs::{Member, Position};
use crate::design::scope::{DesignScope, OccurrenceDesign};
use crate::design::selection::{OccurrencePath, PathStep, StepRef};
use crate::features::ConfigurationId;
use crate::ids::{OccurrenceId, ProductDefinitionId};
use crate::presentation::{PresentationItem, PresentationLayer};
use crate::products::{NestedOverride, Occurrence, ProductDefinition, Storage};
use crate::references::InstanceTarget;
use crate::topology::Color;
use crate::transform::Transform;
use crate::CadIr;
use cadmpeg_core::text::NonBlankString;

fn id<T>(kind: &str, key: u32) -> T
where
    T: TryFrom<String>,
    T::Error: Debug,
{
    T::try_from(format!("test:effective:{kind}#{key}")).unwrap()
}

fn definition(key: u32) -> ProductDefinitionId {
    id("product-definition", key)
}

fn occurrence(key: u32) -> OccurrenceId {
    id("occurrence", key)
}

fn configuration(key: u32) -> ConfigurationId {
    id("configuration", key)
}

fn shift(x: f64, y: f64, z: f64) -> Transform {
    Transform::affine([[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, y], [0.0, 0.0, 1.0, z]]).unwrap()
}

fn translation(transform: Transform) -> [f64; 3] {
    let rows = transform.affine_rows();
    [rows[0][3], rows[1][3], rows[2][3]]
}

fn path(keys: &[u32]) -> OccurrencePath {
    OccurrencePath(
        keys.iter()
            .map(|key| PathStep {
                occurrence: StepRef::Occurrence(occurrence(*key)),
                configuration: None,
            })
            .collect(),
    )
}

fn scope(key: u32, active: Option<u32>) -> DesignScope {
    DesignScope {
        definition: definition(key),
        roots: Vec::new(),
        active_configuration: active.map(configuration),
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: Vec::new(),
        unproduced: BTreeSet::new(),
    }
}

fn configuration_of(key: u32, scope: u32) -> DesignConfiguration {
    DesignConfiguration {
        id: configuration(key),
        scope: definition(scope),
        name: format!("c{key}"),
        position: None,
        parent: None,
        comment: None,
        overrides: Overrides::default(),
        active_arrangements: BTreeSet::new(),
        materials: Vec::new(),
        native_ref: None,
    }
}

fn placement(owner: usize, key: u32, prototype: u32, at: Transform) -> (usize, Occurrence) {
    (
        owner,
        Occurrence::new(occurrence(key), definition(prototype), Some(at)),
    )
}

/// Definitions 1 (the root), 2, 3, and 4, each placed once below the one
/// before it: occurrence 1 of 2 in 1 at x 10, occurrence 2 of 3 in 2 at
/// y 1, and occurrence 3 of 4 in 3 at z 100. Definitions 1 and 2 have
/// scopes; 1's active configuration is 11, 2's is 21, and 2 also holds 22.
fn assembly() -> CadIr {
    let mut document = CadIr::empty();
    for key in 1..=4 {
        document
            .model
            .product_definitions
            .push(ProductDefinition::new(definition(key), Storage::Embedded));
    }
    for (owner, record) in [
        placement(1, 1, 2, shift(10.0, 0.0, 0.0)),
        placement(2, 2, 3, shift(0.0, 1.0, 0.0)),
        placement(3, 3, 4, shift(0.0, 0.0, 100.0)),
    ] {
        document.model.product_definitions[owner - 1]
            .occurrences
            .push(record.id.clone());
        document.model.occurrences.push(record);
    }
    document.design.scopes = vec![scope(1, Some(11)), scope(2, Some(21))];
    document.design.configurations = vec![
        configuration_of(11, 1),
        configuration_of(21, 2),
        configuration_of(22, 2),
    ];
    document
}

fn configured(document: &mut CadIr, key: u32) -> &mut Overrides {
    &mut document
        .design
        .configurations
        .iter_mut()
        .find(|candidate| candidate.id == configuration(key))
        .unwrap()
        .overrides
}

fn transform_at(document: &CadIr, keys: &[u32]) -> Option<[f64; 3]> {
    document
        .effective_transform(&path(keys))
        .unwrap()
        .map(translation)
}

#[test]
fn a_path_composes_its_placements_in_document_coordinates() {
    let document = assembly();
    assert_eq!(transform_at(&document, &[1]), Some([10.0, 0.0, 0.0]));
    assert_eq!(
        transform_at(&document, &[1, 2, 3]),
        Some([10.0, 1.0, 100.0])
    );
}

#[test]
fn a_link_with_no_placement_or_a_native_step_has_no_transform() {
    let mut document = assembly();
    document.model.occurrences[1].transform = None;
    assert_eq!(transform_at(&document, &[1, 2, 3]), None);
    assert_eq!(transform_at(&document, &[1]), Some([10.0, 0.0, 0.0]));
    let mut native = path(&[1]);
    native.0.push(PathStep {
        occurrence: StepRef::Native(crate::design::names::NativeName {
            codec: crate::codec_id!("test"),
            value: crate::design::names::NativeNameValue::Text(
                cadmpeg_core::text::NonBlankString::new("n2").unwrap(),
            ),
        }),
        configuration: None,
    });
    assert_eq!(document.effective_transform(&native).unwrap(), None);
}

#[test]
fn an_occurrence_resolves_through_the_active_configuration_of_its_scope() {
    let mut document = assembly();
    configured(&mut document, 11).occurrences.insert(
        occurrence(1),
        OccurrenceOverride {
            transform: Some(shift(20.0, 0.0, 0.0)),
            ..OccurrenceOverride::default()
        },
    );
    assert_eq!(transform_at(&document, &[1, 2]), Some([20.0, 1.0, 0.0]));
}

#[test]
fn a_child_resolves_through_the_configuration_its_parent_selects() {
    let mut document = assembly();
    for (key, y) in [(21, 2.0), (22, 3.0)] {
        configured(&mut document, key).occurrences.insert(
            occurrence(2),
            OccurrenceOverride {
                transform: Some(shift(0.0, y, 0.0)),
                ..OccurrenceOverride::default()
            },
        );
    }
    assert_eq!(transform_at(&document, &[1, 2]), Some([10.0, 2.0, 0.0]));
    document.design.scopes[0].occurrences.insert(
        occurrence(1),
        OccurrenceDesign {
            configuration: Some(ConfigurationRef::Local(configuration(22))),
            arrangements: Vec::new(),
            grounded: None,
            flexible: None,
            parameters: BTreeMap::new(),
        },
    );
    assert_eq!(transform_at(&document, &[1, 2]), Some([10.0, 3.0, 0.0]));
    let mut stated = path(&[1, 2]);
    stated.0[0].configuration = Some(ConfigurationRef::Local(configuration(21)));
    assert_eq!(
        document
            .effective_transform(&stated)
            .unwrap()
            .map(translation),
        Some([10.0, 2.0, 0.0])
    );
}

fn nested(keys: &[u32], transform: Option<Transform>, visible: Option<bool>) -> NestedOverride {
    NestedOverride {
        path: path(keys),
        transform,
        suppressed: None,
        visible,
        representation: None,
    }
}

#[test]
fn where_two_occurrences_above_state_one_field_the_outer_value_holds() {
    let mut document = assembly();
    document.model.occurrences[0]
        .overrides
        .push(nested(&[2, 3], Some(shift(0.0, 0.0, 7.0)), None));
    document.model.occurrences[1].overrides.push(nested(
        &[3],
        Some(shift(0.0, 0.0, 5.0)),
        Some(false),
    ));
    assert_eq!(transform_at(&document, &[1, 2, 3]), Some([10.0, 1.0, 7.0]));
    assert_eq!(transform_at(&document, &[2, 3]), Some([0.0, 1.0, 5.0]));
    let target = |keys: &[u32]| InstanceTarget {
        entity: AppearanceTarget::Body(id("body", 1)),
        path: Some(path(keys)),
        state: None,
    };
    assert!(!document.effective_visibility(&target(&[1, 2, 3])).unwrap());
    document.model.occurrences[0]
        .overrides
        .push(nested(&[2, 3], None, Some(true)));
    assert!(document.effective_visibility(&target(&[1, 2, 3])).unwrap());
    assert!(!document.effective_visibility(&target(&[2, 3])).unwrap());
}

#[test]
fn a_configuration_nested_entry_stands_before_the_base_list() {
    let mut document = assembly();
    document.model.occurrences[0]
        .overrides
        .push(nested(&[2], Some(shift(0.0, 4.0, 0.0)), None));
    assert_eq!(transform_at(&document, &[1, 2]), Some([10.0, 4.0, 0.0]));
    configured(&mut document, 11).occurrences.insert(
        occurrence(1),
        OccurrenceOverride {
            nested: vec![nested(&[2], Some(shift(0.0, 6.0, 0.0)), None)],
            ..OccurrenceOverride::default()
        },
    );
    assert_eq!(transform_at(&document, &[1, 2]), Some([10.0, 6.0, 0.0]));
}

/// `assembly()` with a replacement for definition 2: definition 5, whose
/// occurrence 5 of definition 6 sits at y 7. 5's active configuration 51
/// places occurrence 5 at y 8, and its configuration 52 places it at y 9
/// and hides it. The root also holds configuration 12, which is not active.
fn with_replacement() -> CadIr {
    let mut document = assembly();
    for key in [5, 6] {
        document
            .model
            .product_definitions
            .push(ProductDefinition::new(definition(key), Storage::Embedded));
    }
    let (_, record) = placement(5, 5, 6, shift(0.0, 7.0, 0.0));
    document.model.product_definitions[4]
        .occurrences
        .push(record.id.clone());
    document.model.occurrences.push(record);
    document.design.scopes.push(scope(5, Some(51)));
    for (key, scope) in [(12, 1), (51, 5), (52, 5)] {
        document
            .design
            .configurations
            .push(configuration_of(key, scope));
    }
    for (key, y, visible) in [(51, 8.0, None), (52, 9.0, Some(false))] {
        configured(&mut document, key).occurrences.insert(
            occurrence(5),
            OccurrenceOverride {
                transform: Some(shift(0.0, y, 0.0)),
                visible,
                ..OccurrenceOverride::default()
            },
        );
    }
    document
}

/// Whether the body at `keys` is shown in the current geometry.
fn shown_at(document: &CadIr, keys: &[u32]) -> bool {
    document
        .effective_visibility(&InstanceTarget {
            entity: AppearanceTarget::Body(id("body", 1)),
            path: Some(path(keys)),
            state: None,
        })
        .unwrap()
}

/// The override of configuration `key` that replaces occurrence 1's
/// prototype with definition 5, selecting `selects` of it where stated.
fn replace_in(document: &mut CadIr, key: u32, selects: Option<u32>) {
    configured(document, key).occurrences.insert(
        occurrence(1),
        OccurrenceOverride {
            prototype: Some(definition(5)),
            configuration: selects.map(|key| ConfigurationRef::Local(configuration(key))),
            ..OccurrenceOverride::default()
        },
    );
}

#[test]
fn an_occurrence_whose_prototype_the_active_configuration_replaces_resolves_in_the_replacement() {
    let mut document = with_replacement();
    replace_in(&mut document, 11, Some(52));
    assert_eq!(transform_at(&document, &[1, 5]), Some([10.0, 9.0, 0.0]));
    assert!(!shown_at(&document, &[1, 5]));

    replace_in(&mut document, 11, None);
    assert_eq!(transform_at(&document, &[1, 5]), Some([10.0, 8.0, 0.0]));
    assert!(shown_at(&document, &[1, 5]));
}

#[test]
fn an_occurrence_whose_prototype_only_another_configuration_replaces_keeps_its_prototype() {
    let mut document = with_replacement();
    replace_in(&mut document, 12, Some(52));
    configured(&mut document, 21).occurrences.insert(
        occurrence(2),
        OccurrenceOverride {
            transform: Some(shift(0.0, 2.0, 0.0)),
            visible: Some(false),
            ..OccurrenceOverride::default()
        },
    );
    assert_eq!(transform_at(&document, &[1, 2]), Some([10.0, 2.0, 0.0]));
    assert!(!shown_at(&document, &[1, 2]));
}

#[test]
fn a_base_selection_of_the_replaced_prototype_resolves_as_no_selection() {
    let mut document = with_replacement();
    replace_in(&mut document, 11, None);
    document.design.scopes[0].occurrences.insert(
        occurrence(1),
        OccurrenceDesign {
            configuration: Some(ConfigurationRef::Local(configuration(22))),
            arrangements: Vec::new(),
            grounded: None,
            flexible: None,
            parameters: BTreeMap::new(),
        },
    );
    assert_eq!(transform_at(&document, &[1, 5]), Some([10.0, 8.0, 0.0]));
    assert!(shown_at(&document, &[1, 5]));
}

#[test]
fn a_replacement_in_a_parent_configuration_applies_until_the_child_states_its_own() {
    let mut document = with_replacement();
    replace_in(&mut document, 12, Some(52));
    document
        .design
        .configurations
        .iter_mut()
        .find(|candidate| candidate.id == configuration(11))
        .unwrap()
        .parent = Some(configuration(12));
    assert_eq!(transform_at(&document, &[1, 5]), Some([10.0, 9.0, 0.0]));
    assert!(!shown_at(&document, &[1, 5]));

    configured(&mut document, 11).occurrences.insert(
        occurrence(1),
        OccurrenceOverride {
            prototype: Some(definition(2)),
            configuration: Some(ConfigurationRef::Local(configuration(22))),
            ..OccurrenceOverride::default()
        },
    );
    configured(&mut document, 22).occurrences.insert(
        occurrence(2),
        OccurrenceOverride {
            transform: Some(shift(0.0, 3.0, 0.0)),
            ..OccurrenceOverride::default()
        },
    );
    assert_eq!(transform_at(&document, &[1, 2]), Some([10.0, 3.0, 0.0]));
}

fn posed(keys: &[u32], at: Transform) -> CurrentState {
    CurrentState {
        at: Position::After(Member::Feature(id("feature", 1))),
        bodies: Vec::new(),
        poses: BTreeMap::from([(path(keys), at)]),
        status: EvaluationStatus::Current,
        native_ref: None,
    }
}

#[test]
fn a_current_pose_places_an_instance_only_in_the_active_configuration() {
    let mut document = assembly();
    document.design.scopes[1]
        .current
        .push(posed(&[2], shift(0.0, 9.0, 0.0)));
    assert_eq!(transform_at(&document, &[1, 2]), Some([10.0, 9.0, 0.0]));
    let mut other = path(&[1, 2]);
    other.0[0].configuration = Some(ConfigurationRef::Local(configuration(22)));
    assert_eq!(
        document
            .effective_transform(&other)
            .unwrap()
            .map(translation),
        Some([10.0, 1.0, 0.0])
    );
}

#[test]
fn a_scope_that_spans_a_definition_poses_its_nested_paths() {
    let mut document = assembly();
    document.design.scopes.truncate(1);
    document.design.scopes[0]
        .current
        .push(posed(&[1, 2], shift(0.0, 8.0, 0.0)));
    assert_eq!(transform_at(&document, &[1, 2]), Some([10.0, 8.0, 0.0]));
    assert_eq!(transform_at(&document, &[2]), Some([0.0, 1.0, 0.0]));
}

fn cube() -> CadIr {
    let mut document = crate::examples::unit_cube().unwrap();
    let assembly = assembly();
    document.model.product_definitions = assembly.model.product_definitions;
    document.model.occurrences = assembly.model.occurrences;
    document.design.scopes = assembly.design.scopes;
    document.design.configurations = assembly.design.configurations;
    document
}

fn color(r: f64) -> Color {
    Color::new(r, 0.0, 0.0, 1.0).unwrap()
}

fn appearance(key: u32, base: Option<Color>) -> Appearance {
    let mut record: Appearance = json::from_value(cadmpeg_core::json!({
        "id": format!("test:effective:appearance#{key}"),
        "properties": {},
        "textures": [],
    }))
    .unwrap();
    record.base_color = base;
    record
}

fn binding(
    key: u32,
    target: InstanceTarget<AppearanceTarget>,
    appearance: u32,
) -> AppearanceBinding {
    AppearanceBinding {
        id: id("appearance-binding", key),
        target,
        appearance: id("appearance", appearance),
        precedence: None,
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: BTreeMap::new(),
        native_ref: None,
    }
}

#[test]
fn a_display_colour_is_explicit_then_bound_then_the_bodys() {
    let mut document = cube();
    let body = document.geometry.bodies[0].id.clone();
    let face = document.geometry.faces[0].id.clone();
    let on_face = InstanceTarget::new(AppearanceTarget::Face(face.clone()));
    let on_body = InstanceTarget::new(AppearanceTarget::Body(body.clone()));
    document.geometry.bodies[0].color = None;
    for face in &mut document.geometry.faces {
        face.color = None;
    }
    assert_eq!(document.effective_display_color(&on_face).unwrap(), None);
    document.model.appearances = vec![
        appearance(1, Some(color(0.1))),
        appearance(2, Some(color(0.2))),
        appearance(3, Some(color(0.3))),
    ];
    document.model.appearance_bindings = vec![binding(1, on_body.clone(), 1)];
    assert_eq!(
        document.effective_display_color(&on_face).unwrap(),
        Some(color(0.1))
    );
    document.geometry.bodies[0].color = Some(color(0.4));
    assert_eq!(
        document.effective_display_color(&on_face).unwrap(),
        Some(color(0.4))
    );
    document
        .model
        .appearance_bindings
        .push(binding(2, on_face.clone(), 2));
    assert_eq!(
        document.effective_display_color(&on_face).unwrap(),
        Some(color(0.2))
    );
    let placed = InstanceTarget {
        path: Some(path(&[1])),
        ..on_face.clone()
    };
    document
        .model
        .appearance_bindings
        .push(binding(3, placed.clone(), 3));
    assert_eq!(
        document.effective_display_color(&placed).unwrap(),
        Some(color(0.3))
    );
    assert_eq!(
        document.effective_display_color(&on_face).unwrap(),
        Some(color(0.2))
    );
    document.geometry.faces[0].color = Some(color(0.5));
    assert_eq!(
        document.effective_display_color(&placed).unwrap(),
        Some(color(0.5))
    );
    assert_eq!(
        document.effective_display_color(&on_body).unwrap(),
        Some(color(0.4))
    );
}

#[test]
fn visibility_is_every_stated_gate_combined_with_and() {
    let mut document = cube();
    let body = document.geometry.bodies[0].id.clone();
    let face = document.geometry.faces[0].id.clone();
    let placed = InstanceTarget {
        entity: AppearanceTarget::Face(face),
        path: Some(path(&[1, 2])),
        state: None,
    };
    let shown = |document: &CadIr| document.effective_visibility(&placed).unwrap();
    assert!(shown(&document));

    document.geometry.bodies[0].visible = Some(false);
    assert!(!shown(&document));
    document.geometry.bodies[0].visible = Some(true);

    configured(&mut document, 21).occurrences.insert(
        occurrence(2),
        OccurrenceOverride {
            visible: Some(false),
            ..OccurrenceOverride::default()
        },
    );
    assert!(!shown(&document));
    configured(&mut document, 21).occurrences.clear();

    document.model.appearances = vec![appearance(1, None)];
    let mut hidden = binding(
        1,
        InstanceTarget::new(AppearanceTarget::Body(body.clone())),
        1,
    );
    hidden.visible = Some(false);
    document.model.appearance_bindings = vec![hidden];
    assert!(!shown(&document));
    document.model.appearance_bindings.clear();

    let layer = |items| PresentationLayer {
        id: id("layer", 1),
        name: "l".into(),
        parent: None,
        description: None,
        visible: Some(false),
        items,
        native_ref: None,
    };
    document.model.presentation_layers =
        vec![layer(vec![InstanceTarget::new(PresentationItem::Body {
            body,
        })])];
    assert!(!shown(&document));
    document.model.presentation_layers = vec![layer(vec![InstanceTarget::new(
        PresentationItem::Occurrence {
            occurrence: occurrence(1),
        },
    )])];
    assert!(!shown(&document));
    document.model.presentation_layers[0].visible = Some(true);
    assert!(shown(&document));
}

/// A hidden layer hides the items of the layers below it through `parent`.
#[test]
fn a_hidden_parent_layer_hides_its_sublayers_items() {
    let mut document = cube();
    let body = document.geometry.bodies[0].id.clone();
    let layer = |key: u32, parent: Option<u32>, visible, items| PresentationLayer {
        id: id("layer", key),
        name: format!("l{key}"),
        parent: parent.map(|key| id("layer", key)),
        description: None,
        visible,
        items,
        native_ref: None,
    };
    document.model.presentation_layers = vec![
        layer(1, None, Some(false), Vec::new()),
        layer(2, Some(1), None, Vec::new()),
        layer(
            3,
            Some(2),
            Some(true),
            vec![InstanceTarget::new(PresentationItem::Body {
                body: body.clone(),
            })],
        ),
    ];
    let target = InstanceTarget::new(AppearanceTarget::Body(body));
    assert!(!document.effective_visibility(&target).unwrap());
    document.model.presentation_layers[0].visible = Some(true);
    assert!(document.effective_visibility(&target).unwrap());
}

fn arrangement(key: u32, scope: u32, overrides: Overrides) -> Arrangement {
    Arrangement {
        id: id("arrangement", key),
        scope: definition(scope),
        name: NonBlankString::new(format!("a{key}")).unwrap(),
        kind: ArrangementKind::Positional,
        configuration: None,
        overrides,
        appearances: Vec::new(),
        moves: Vec::new(),
        nested: Vec::new(),
        native_ref: None,
    }
}

fn moves_occurrence_2(y: f64, visible: Option<bool>) -> Overrides {
    let mut overrides = Overrides::default();
    overrides.occurrences.insert(
        occurrence(2),
        OccurrenceOverride {
            transform: Some(shift(0.0, y, 0.0)),
            visible,
            ..OccurrenceOverride::default()
        },
    );
    overrides
}

/// An arrangement in force in definition 1 chooses, by a `nested` entry, the
/// arrangement of definition 2 that occurrence 1 shows, in place of the one
/// occurrence 1's own design names; a named entry chooses none.
#[test]
fn a_nested_arrangement_is_in_force_for_the_instance_it_names() {
    let mut document = assembly();
    let mut outer = arrangement(1, 1, Overrides::default());
    outer.nested.push(NestedArrangement {
        path: path(&[1]),
        arrangement: ArrangementRef::Local(id("arrangement", 2)),
    });
    document.design.arrangements = vec![
        outer,
        arrangement(2, 2, moves_occurrence_2(5.0, Some(false))),
        arrangement(3, 2, moves_occurrence_2(3.0, None)),
    ];
    document.design.scopes[0].occurrences.insert(
        occurrence(1),
        OccurrenceDesign {
            configuration: None,
            arrangements: vec![ArrangementRef::Local(id("arrangement", 3))],
            grounded: None,
            flexible: None,
            parameters: BTreeMap::new(),
        },
    );
    assert_eq!(transform_at(&document, &[1, 2]), Some([10.0, 3.0, 0.0]));

    document.design.scopes[0]
        .active_arrangements
        .insert(id("arrangement", 1));
    assert_eq!(transform_at(&document, &[1, 2]), Some([10.0, 5.0, 0.0]));
    let placed = InstanceTarget {
        entity: AppearanceTarget::Body(id("body", 1)),
        path: Some(path(&[1, 2])),
        state: None,
    };
    assert!(!document.effective_visibility(&placed).unwrap());

    document.design.arrangements[0].nested[0].arrangement =
        ArrangementRef::Named(NonBlankString::new("a2".to_owned()).unwrap());
    assert_eq!(transform_at(&document, &[1, 2]), Some([10.0, 3.0, 0.0]));
}
