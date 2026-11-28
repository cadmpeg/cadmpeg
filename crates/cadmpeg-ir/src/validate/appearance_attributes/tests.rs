// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_core::decode::ContentScope;

use crate::appearance::{Appearance, AppearanceBinding, AppearancePrecedence, AppearanceTarget};
use crate::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use crate::examples::unit_cube;
use crate::features::ParameterId;
use crate::ids::{AppearanceId, AttributeId};
use crate::report::check::Check;
use crate::validate::validate_neutral;
use crate::CadIr;

const ATTRIBUTE: &str = "test:model:attribute#1";
const BINDING: &str = "test:model:appearance-binding#1";

fn attribute(target: AttributeTarget, values: Vec<Option<AttributeValue>>) -> SourceAttribute {
    SourceAttribute {
        id: AttributeId::mint(ATTRIBUTE).unwrap(),
        target: target.into(),
        name: "Property".into(),
        values,
        native_ref: None,
    }
}

fn appearance() -> Appearance {
    Appearance {
        id: AppearanceId::mint("test:model:appearance#1").unwrap(),
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
        native_ref: None,
    }
}

fn binding(target: AppearanceTarget) -> AppearanceBinding {
    AppearanceBinding {
        id: BINDING.try_into().unwrap(),
        target: target.into(),
        appearance: appearance().id,
        precedence: None,
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: BTreeMap::new(),
        native_ref: None,
    }
}

/// The messages of the design findings on `entity`.
fn design_findings(ir: &CadIr, entity: &str) -> Vec<String> {
    validate_neutral(ir, Vec::new())
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| {
            finding.check == Check::Design && finding.entity.as_deref() == Some(entity)
        })
        .map(|finding| finding.message)
        .collect()
}

fn with_attribute(target: AttributeTarget, values: Vec<Option<AttributeValue>>) -> Vec<String> {
    let mut ir = unit_cube().unwrap();
    ir.model.attributes.push(attribute(target, values));
    design_findings(&ir, ATTRIBUTE)
}

#[test]
fn each_target_arm_names_a_record_of_its_kind() {
    let id = |kind: &str| format!("test:model:{kind}#missing");
    let cases: Vec<(AttributeTarget, &str, String)> = vec![
        (
            AttributeTarget::Shell(id("shell").as_str().try_into().unwrap()),
            "shell",
            id("shell"),
        ),
        (
            AttributeTarget::Loop(id("loop").as_str().try_into().unwrap()),
            "loop",
            id("loop"),
        ),
        (
            AttributeTarget::Region(id("region").as_str().try_into().unwrap()),
            "region",
            id("region"),
        ),
        (
            AttributeTarget::Surface(id("surface").as_str().try_into().unwrap()),
            "surface",
            id("surface"),
        ),
        (
            AttributeTarget::Curve(id("curve").as_str().try_into().unwrap()),
            "curve",
            id("curve"),
        ),
        (
            AttributeTarget::Point(id("point").as_str().try_into().unwrap()),
            "point",
            id("point"),
        ),
        (
            AttributeTarget::Feature(crate::features::FeatureId::mint(id("feature")).unwrap()),
            "feature",
            id("feature"),
        ),
        (
            AttributeTarget::Sketch(crate::sketches::SketchId::mint(id("sketch")).unwrap()),
            "sketch",
            id("sketch"),
        ),
        (
            AttributeTarget::Parameter(ParameterId::mint(id("parameter")).unwrap()),
            "parameter",
            id("parameter"),
        ),
        (
            AttributeTarget::ProductDefinition(id("definition").as_str().try_into().unwrap()),
            "product definition",
            id("definition"),
        ),
        (
            AttributeTarget::Occurrence(id("occurrence").as_str().try_into().unwrap()),
            "occurrence",
            id("occurrence"),
        ),
        (
            AttributeTarget::Tessellation(id("tessellation").try_into().unwrap()),
            "tessellation",
            id("tessellation"),
        ),
        (
            AttributeTarget::Pmi(id("pmi").as_str().try_into().unwrap()),
            "PMI annotation",
            id("pmi"),
        ),
        (
            AttributeTarget::SemanticAnnotation(id("annotation").as_str().try_into().unwrap()),
            "semantic annotation",
            id("annotation"),
        ),
        (
            AttributeTarget::Drawing(id("drawing").as_str().try_into().unwrap()),
            "drawing",
            id("drawing"),
        ),
        (
            AttributeTarget::ViewPresentation(id("view").as_str().try_into().unwrap()),
            "view presentation",
            id("view"),
        ),
    ];
    for (target, kind, id) in cases {
        assert_eq!(
            with_attribute(target, Vec::new()),
            [format!("references missing {kind} `{id}`")]
        );
    }
    let ir = unit_cube().unwrap();
    let geometry = &ir.geometry;
    for target in [
        AttributeTarget::Shell(geometry.shells[0].id.clone()),
        AttributeTarget::Loop(geometry.loops[0].id.clone()),
        AttributeTarget::Region(geometry.regions[0].id.clone()),
        AttributeTarget::Surface(geometry.surfaces[0].id.clone()),
        AttributeTarget::Curve(geometry.curves[0].id.clone()),
        AttributeTarget::Point(geometry.points[0].id.clone()),
    ] {
        assert_eq!(with_attribute(target, Vec::new()), Vec::<String>::new());
    }
}

#[test]
fn a_record_target_names_an_arena_and_an_id_of_it() {
    let ir = unit_cube().unwrap();
    let body = ir.geometry.bodies[0].id.to_string();
    let record = |arena: &str, id: &str| AttributeTarget::Record {
        arena: cadmpeg_core::text::NonBlankString::new(arena).unwrap(),
        id: id.try_into().unwrap(),
    };
    assert_eq!(
        with_attribute(record("bodies", &body), Vec::new()),
        Vec::<String>::new()
    );
    assert_eq!(
        with_attribute(record("attributes", ATTRIBUTE), Vec::new()),
        Vec::<String>::new()
    );
    assert_eq!(
        with_attribute(record("faces", &body), Vec::new()),
        [format!(
            "record target `{body}` is no record of arena `faces`"
        )]
    );
    for arena in ["setups", "geometry", "Bodies"] {
        assert_eq!(
            with_attribute(record(arena, &body), Vec::new()),
            [format!("record target names `{arena}`, which is no arena of the model, the design, or the topology")]
        );
    }

    let mut ir = unit_cube().unwrap();
    ir.model.appearances.push(appearance());
    ir.model
        .appearance_bindings
        .push(binding(AppearanceTarget::Record {
            arena: cadmpeg_core::text::NonBlankString::new("point_clouds").unwrap(),
            id: "test:model:cloud#1".try_into().unwrap(),
        }));
    assert_eq!(
        design_findings(&ir, BINDING),
        ["record target `test:model:cloud#1` is no record of arena `point_clouds`"]
    );
    ir.model.appearance_bindings[0] = binding(AppearanceTarget::Occurrence(
        "test:model:occurrence#1".try_into().unwrap(),
    ));
    ir.model.appearance_bindings[0].precedence = Some(AppearancePrecedence::Outer);
    assert_eq!(
        design_findings(&ir, BINDING),
        ["references missing occurrence `test:model:occurrence#1`"]
    );
}

#[test]
fn an_unresolved_binding_states_what_the_decoder_read_and_no_instance() {
    let findings = |edit: &dyn Fn(&mut AppearanceBinding)| {
        let mut ir = unit_cube().unwrap();
        ir.model.appearances.push(appearance());
        let mut assigned = binding(AppearanceTarget::Unresolved);
        assigned.source_entity_id = Some("#7".into());
        edit(&mut assigned);
        ir.model.appearance_bindings.push(assigned);
        design_findings(&ir, BINDING)
    };
    assert!(findings(&|_| {}).is_empty());
    assert_eq!(
        findings(&|binding| binding.source_entity_id = None),
        ["an unresolved target states neither source_entity_id nor native_ref"]
    );
    assert_eq!(
        findings(&|binding| binding.source_entity_id = Some(" ".into())),
        ["source_entity_id is blank"]
    );
    assert_eq!(
        findings(&|binding| {
            binding.target.state = Some(crate::design::refs::StateRef {
                configuration: None,
                of: crate::design::refs::Evaluated::Tool(
                    crate::features::FeatureId::mint("test:design:feature#1").unwrap(),
                ),
            });
        }),
        ["an unresolved target states a path or a state"]
    );
}

#[test]
fn native_refs_of_appearances_bindings_and_attributes_resolve() {
    let mut ir = unit_cube().unwrap();
    ir.native.namespace_mut(crate::codec_id!("native"));
    let mut asset = appearance();
    asset.native_ref = Some("native:missing#1".into());
    let mut assigned = binding(AppearanceTarget::Body(ir.geometry.bodies[0].id.clone()));
    assigned.native_ref = Some("native:missing#2".into());
    let mut property = attribute(AttributeTarget::Document, Vec::new());
    property.native_ref = Some("native:missing#3".into());
    ir.model.appearances.push(asset);
    ir.model.appearance_bindings.push(assigned);
    ir.model.attributes.push(property);
    for (entity, target) in [
        ("test:model:appearance#1", "native:missing#1"),
        (BINDING, "native:missing#2"),
        (ATTRIBUTE, "native:missing#3"),
    ] {
        assert_eq!(
            design_findings(&ir, entity),
            [format!("native_ref `{target}` does not resolve")]
        );
    }
}

#[test]
fn an_attribute_on_a_design_record_or_reading_a_parameter_needs_the_design_class() {
    let mass = ParameterId::mint("test:design:parameter#mass").unwrap();
    let scoped = |target, values| {
        let mut ir = CadIr::empty();
        ir.set_scope(ContentScope::geometry());
        ir.model.attributes.push(attribute(target, values));
        ir.scope_violations()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    };
    let violation = vec![
        "the document's scope excludes design but model arena `attributes` holds a reference into it"
            .to_owned(),
    ];
    for target in [
        AttributeTarget::Feature(
            crate::features::FeatureId::mint("test:design:feature#1").unwrap(),
        ),
        AttributeTarget::Sketch(crate::sketches::SketchId::mint("test:design:sketch#1").unwrap()),
        AttributeTarget::Parameter(mass.clone()),
    ] {
        assert_eq!(scoped(target, Vec::new()), violation);
    }
    assert_eq!(
        scoped(
            AttributeTarget::Document,
            vec![Some(AttributeValue::Parameter(mass))]
        ),
        violation
    );
    assert!(scoped(AttributeTarget::Document, vec![None]).is_empty());
}

/// The unit cube with an assembly that places a part, and the appearance.
fn assembly() -> CadIr {
    use crate::products::{ProductDefinition, Storage};
    let mut ir = unit_cube().unwrap();
    ir.model.appearances.push(appearance());
    let mut assembly = ProductDefinition::new(
        "test:model:product#assembly".try_into().unwrap(),
        Storage::Embedded,
    );
    assembly
        .occurrences
        .push("test:model:occurrence#part".try_into().unwrap());
    let part = ProductDefinition::new(
        "test:model:product#part".try_into().unwrap(),
        Storage::Embedded,
    );
    ir.model.product_definitions.extend([assembly, part]);
    ir
}

/// The design findings on a binding of `target`, at `path`, that states
/// `precedence`, that concern the precedence or the definition.
fn precedence_findings(
    target: AppearanceTarget,
    path: bool,
    precedence: Option<AppearancePrecedence>,
) -> Vec<String> {
    let mut ir = assembly();
    let mut stated = binding(target);
    if path {
        stated.target.path = Some(crate::design::selection::OccurrencePath(vec![
            crate::design::selection::PathStep {
                occurrence: crate::design::selection::StepRef::Occurrence(
                    "test:model:occurrence#part".try_into().unwrap(),
                ),
                configuration: None,
            },
        ]));
    }
    stated.precedence = precedence;
    ir.model.appearance_bindings.push(stated);
    design_findings(&ir, BINDING)
        .into_iter()
        .filter(|message| message.contains("precedence") || message.contains("definition"))
        .collect()
}

#[test]
fn a_binding_states_precedence_where_a_placed_definition_can_state_a_nearer_target() {
    let body = || AppearanceTarget::Body(unit_cube().unwrap().geometry.bodies[0].id.clone());
    let assembly =
        || AppearanceTarget::Definition("test:model:product#assembly".try_into().unwrap());
    let part = || AppearanceTarget::Definition("test:model:product#part".try_into().unwrap());
    let none: Vec<String> = Vec::new();
    let unstated =
        ["states no precedence on a target a placed definition can state a nearer target under"];
    let stated =
        ["states a precedence on a target no placed definition states a nearer target under"];

    assert_eq!(precedence_findings(body(), false, None), none);
    assert_eq!(
        precedence_findings(body(), false, Some(AppearancePrecedence::Outer)),
        stated
    );
    assert_eq!(precedence_findings(body(), true, None), unstated);
    assert_eq!(
        precedence_findings(body(), true, Some(AppearancePrecedence::Nearest)),
        none
    );
    assert_eq!(precedence_findings(assembly(), false, None), unstated);
    assert_eq!(
        precedence_findings(assembly(), false, Some(AppearancePrecedence::Outer)),
        none
    );
    assert_eq!(precedence_findings(part(), false, None), none);
    assert_eq!(
        precedence_findings(part(), true, None),
        ["a definition target states a path"]
    );
}

#[test]
fn a_definition_target_names_a_product_definition() {
    let mut ir = assembly();
    ir.model
        .appearance_bindings
        .push(binding(AppearanceTarget::Definition(
            "test:model:product#missing".try_into().unwrap(),
        )));
    assert_eq!(
        design_findings(&ir, BINDING),
        ["references missing product definition `test:model:product#missing`"]
    );
}

#[test]
fn a_record_target_names_no_arena_another_arm_names() {
    let mut ir = assembly();
    let body = ir.geometry.bodies[0].id.to_string();
    ir.model
        .appearance_bindings
        .push(binding(AppearanceTarget::Record {
            arena: cadmpeg_core::nonblank_const!("bodies"),
            id: body.try_into().unwrap(),
        }));
    assert_eq!(
        design_findings(&ir, BINDING),
        ["record target names `bodies`, whose records another target arm names"]
    );
}

#[test]
fn two_bindings_of_one_target_with_different_appearances_are_a_warning() {
    let mut ir = assembly();
    let mut other = appearance();
    other.id = AppearanceId::mint("test:model:appearance#2").unwrap();
    ir.model.appearances.push(other.clone());
    let target = AppearanceTarget::Face(ir.geometry.faces[0].id.clone());
    let first = binding(target.clone());
    let mut second = binding(target);
    second.id = "test:model:appearance-binding#2".try_into().unwrap();
    second.appearance = other.id;
    ir.model.appearance_bindings.extend([first, second]);
    let findings = validate_neutral(&ir, Vec::new()).unwrap().findings;
    let warned: Vec<_> = findings
        .iter()
        .filter(|finding| {
            finding.severity == crate::report::Severity::Warning
                && finding.message.contains("same target, path, and state")
        })
        .map(|finding| finding.entity.as_deref())
        .collect();
    assert_eq!(warned, [Some("test:model:appearance-binding#2")]);
}

#[test]
fn two_unresolved_bindings_with_different_appearances_name_no_one_target() {
    let mut ir = assembly();
    let mut other = appearance();
    other.id = AppearanceId::mint("test:model:appearance#2").unwrap();
    ir.model.appearances.push(other.clone());
    let mut first = binding(AppearanceTarget::Unresolved);
    first.source_entity_id = Some("#10".into());
    let mut second = binding(AppearanceTarget::Unresolved);
    second.id = "test:model:appearance-binding#2".try_into().unwrap();
    second.source_entity_id = Some("#20".into());
    second.appearance = other.id;
    ir.model.appearance_bindings.extend([first, second]);
    let findings = validate_neutral(&ir, Vec::new()).unwrap().findings;
    assert!(
        findings
            .iter()
            .all(|finding| !finding.message.contains("same target, path, and state")),
        "{findings:?}"
    );
}
