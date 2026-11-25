// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
//! Part 10, Products (§8.1): each rule broken once by an invented document,
//! and the document that obeys it.

use cadmpeg_core::decode::ContentScope;
use cadmpeg_core::text::NonBlankString;

use crate::design::configuration::OccurrenceOverride;
use crate::design::refs::ConfigurationRef;
use crate::design::selection::{OccurrencePath, PathStep, StepRef};
use crate::document::CadIr;
use crate::ids::{OccurrenceId, ProductDefinitionId};
use crate::products::{
    ExternalDocument, GeneratedFrom, InstanceBodies, MemberAction, MemberRule, NestedOverride,
    Occurrence, ProductDefinition, Representation, RepresentationContent, RepresentationMember,
    Storage,
};
use crate::report::Severity;
use crate::transform::Transform;
use crate::validate::design::fixture::{self, id};
use crate::validate::validate_neutral;

fn occurrence(key: &str) -> OccurrenceId {
    id("occurrence", key)
}

fn definition(key: &str) -> ProductDefinitionId {
    id("definition", key)
}

fn path(keys: &[&str]) -> OccurrencePath {
    OccurrencePath(
        keys.iter()
            .map(|key| PathStep {
                occurrence: StepRef::Occurrence(occurrence(key)),
                configuration: None,
            })
            .collect(),
    )
}

fn name(text: &str) -> NonBlankString {
    NonBlankString::new(text).unwrap()
}

fn representation(text: &str, content: RepresentationContent) -> Representation {
    Representation {
        name: name(text),
        content,
        features: Vec::new(),
        default: None,
        native_ref: None,
    }
}

/// The design fixture's part, placed by occurrence `b` in definition `sub`,
/// which occurrence `a` places in definition `assembly`.
fn assembly() -> CadIr {
    let mut ir = fixture::document();
    let mut assembly = ProductDefinition::new(definition("assembly"), Storage::Embedded);
    assembly.occurrences.push(occurrence("a"));
    let mut sub = ProductDefinition::new(definition("sub"), Storage::Embedded);
    sub.occurrences.push(occurrence("b"));
    ir.model.product_definitions.extend([assembly, sub]);
    ir.model
        .product_definitions
        .sort_by(|left, right| left.id.cmp(&right.id));
    ir.model.occurrences.extend([
        Occurrence::new(occurrence("a"), definition("sub"), None),
        Occurrence::new(occurrence("b"), definition("part"), None),
    ]);
    ir
}

fn definition_mut<'a>(ir: &'a mut CadIr, key: &str) -> &'a mut ProductDefinition {
    let id = definition(key);
    ir.model
        .product_definitions
        .iter_mut()
        .find(|definition| definition.id == id)
        .unwrap()
}

fn occurrence_mut<'a>(ir: &'a mut CadIr, key: &str) -> &'a mut Occurrence {
    let id = occurrence(key);
    ir.model
        .occurrences
        .iter_mut()
        .find(|occurrence| occurrence.id == id)
        .unwrap()
}

/// The severity and entity of each finding whose message holds `fragment`.
fn found(ir: &CadIr, fragment: &str) -> Vec<(Severity, Option<String>)> {
    validate_neutral(ir, Vec::new())
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| finding.message.contains(fragment))
        .map(|finding| (finding.severity, finding.entity))
        .collect()
}

fn error_on(key: &str, kind: &str) -> Vec<(Severity, Option<String>)> {
    vec![(Severity::Error, Some(format!("test:design:{kind}#{key}")))]
}

/// The part with a representation named `simplified`.
fn with_simplified() -> CadIr {
    let mut ir = assembly();
    let body = fixture::body(&ir);
    definition_mut(&mut ir, "part")
        .representations
        .push(representation(
            "simplified",
            RepresentationContent::Bodies(vec![body]),
        ));
    ir
}

#[test]
fn an_external_definition_holds_no_bodies_occurrences_or_scope() {
    const FRAGMENT: &str = "an External definition holds";
    let mut ir = assembly();
    ir.model.product_definitions.push(ProductDefinition::new(
        definition("unread"),
        Storage::External(ExternalDocument::path("unread.part")),
    ));
    assert!(found(&ir, FRAGMENT).is_empty());
    definition_mut(&mut ir, "unread")
        .occurrences
        .push(occurrence("b"));
    assert_eq!(found(&ir, FRAGMENT), error_on("unread", "definition"));

    let mut scoped = assembly();
    definition_mut(&mut scoped, "part").storage =
        Storage::External(ExternalDocument::path("part.part"));
    definition_mut(&mut scoped, "part").bodies.clear();
    assert_eq!(found(&scoped, FRAGMENT), error_on("part", "definition"));
}

#[test]
fn an_occurrence_names_a_representation_of_its_prototype() {
    const FRAGMENT: &str = "is no representation of";
    let mut ir = with_simplified();
    occurrence_mut(&mut ir, "b").representation = Some(name("simplified"));
    assert!(found(&ir, FRAGMENT).is_empty());
    occurrence_mut(&mut ir, "b").representation = Some(name("full"));
    assert_eq!(found(&ir, FRAGMENT), error_on("b", "occurrence"));
}

#[test]
fn a_nested_override_names_a_representation_of_the_prototype_its_path_reaches() {
    const FRAGMENT: &str = "is no representation of";
    let nested = |text: &str| NestedOverride {
        path: path(&["b"]),
        transform: None,
        suppressed: None,
        visible: None,
        representation: Some(name(text)),
    };
    let mut ir = with_simplified();
    occurrence_mut(&mut ir, "a")
        .overrides
        .push(nested("simplified"));
    assert!(found(&ir, FRAGMENT).is_empty());
    occurrence_mut(&mut ir, "a").overrides[0] = nested("full");
    assert_eq!(found(&ir, FRAGMENT), error_on("a", "occurrence"));
}

#[test]
fn a_configuration_override_names_a_representation_of_its_prototype() {
    const FRAGMENT: &str = "is no representation of";
    let with = |text: &str| {
        let mut ir = with_simplified();
        ir.design.configurations[0].overrides.occurrences.insert(
            occurrence("b"),
            OccurrenceOverride {
                representation: Some(name(text)),
                ..OccurrenceOverride::default()
            },
        );
        ir
    };
    assert!(found(&with("simplified"), FRAGMENT).is_empty());
    assert_eq!(
        found(&with("full"), FRAGMENT),
        error_on("base", "configuration")
    );
}

#[test]
fn instance_body_paths_resolve_from_the_owning_definition() {
    const FRAGMENT: &str = "is not an occurrence of the previous step's prototype";
    let with = |keys: &[&str]| {
        let mut ir = assembly();
        definition_mut(&mut ir, "assembly")
            .instance_bodies
            .push(InstanceBodies {
                occurrence: path(keys),
                bodies: Vec::new(),
            });
        ir
    };
    assert!(found(&with(&["a", "b"]), FRAGMENT).is_empty());
    assert_eq!(
        found(&with(&["b"]), FRAGMENT),
        error_on("assembly", "definition")
    );
}

#[test]
fn a_definition_has_at_most_one_default_representation() {
    const FRAGMENT: &str = "representations of the definition are its default";
    let mut ir = with_simplified();
    let mut other = representation("light", RepresentationContent::Unread);
    other.default = Some(true);
    definition_mut(&mut ir, "part").representations.push(other);
    assert!(found(&ir, FRAGMENT).is_empty());
    definition_mut(&mut ir, "part").representations[0].default = Some(true);
    assert_eq!(found(&ir, FRAGMENT), error_on("part", "definition"));
}

#[test]
fn a_bodies_representation_names_bodies_of_the_definition() {
    const FRAGMENT: &str = "which is no body of the definition";
    let ir = with_simplified();
    assert!(found(&ir, FRAGMENT).is_empty());
    let mut moved = ir.clone();
    let content = definition_mut(&mut moved, "part").representations[0]
        .content
        .clone();
    definition_mut(&mut moved, "sub")
        .representations
        .push(representation("simplified", content));
    assert_eq!(found(&moved, FRAGMENT), error_on("sub", "definition"));
}

/// The assembly with a member rule over instance `a`, and the sub-assembly
/// with a representation named `simplified`.
fn with_members(unlisted: Option<MemberAction>, action: MemberAction) -> CadIr {
    let mut ir = assembly();
    definition_mut(&mut ir, "sub")
        .representations
        .push(representation("simplified", RepresentationContent::Unread));
    definition_mut(&mut ir, "assembly")
        .representations
        .push(representation(
            "rule",
            RepresentationContent::Members(MemberRule {
                unlisted,
                members: vec![RepresentationMember {
                    path: path(&["a"]),
                    action,
                }],
            }),
        ));
    ir
}

#[test]
fn an_unlisted_action_is_no_substitution() {
    const FRAGMENT: &str = "applies a substitution to the instances no member names";
    let ir = with_members(Some(MemberAction::Exclude), MemberAction::Include);
    assert!(found(&ir, FRAGMENT).is_empty());
    let ir = with_members(
        Some(MemberAction::Substitute(name("simplified"))),
        MemberAction::Include,
    );
    assert_eq!(found(&ir, FRAGMENT), error_on("assembly", "definition"));
}

#[test]
fn member_paths_resolve_from_the_definition() {
    const FRAGMENT: &str = "is not an occurrence of the previous step's prototype";
    let ir = with_members(None, MemberAction::Include);
    assert!(found(&ir, FRAGMENT).is_empty());
    let mut ir = ir;
    if let RepresentationContent::Members(rule) =
        &mut definition_mut(&mut ir, "assembly").representations[0].content
    {
        rule.members[0].path = path(&["b"]);
    }
    assert_eq!(found(&ir, FRAGMENT), error_on("assembly", "definition"));
}

#[test]
fn a_substitute_names_a_representation_of_the_members_prototype() {
    const FRAGMENT: &str = "is no representation of";
    let ir = with_members(None, MemberAction::Substitute(name("simplified")));
    assert!(found(&ir, FRAGMENT).is_empty());
    let ir = with_members(None, MemberAction::Substitute(name("full")));
    assert_eq!(found(&ir, FRAGMENT), error_on("assembly", "definition"));
}

#[test]
fn a_replacement_names_other_occurrences_of_the_definition() {
    const NONE: &str = "replaces a member by no occurrence";
    const OWN: &str = "replaces a member by a member's own path";
    let mut ir = with_members(None, MemberAction::ReplaceBy(vec![path(&["c"])]));
    definition_mut(&mut ir, "assembly")
        .occurrences
        .push(occurrence("c"));
    ir.model
        .occurrences
        .push(Occurrence::new(occurrence("c"), definition("part"), None));
    assert!(found(&ir, NONE).is_empty());
    assert!(found(&ir, OWN).is_empty());
    let ir = with_members(None, MemberAction::ReplaceBy(Vec::new()));
    assert_eq!(found(&ir, NONE), error_on("assembly", "definition"));
    let ir = with_members(None, MemberAction::ReplaceBy(vec![path(&["a"])]));
    assert_eq!(found(&ir, OWN), error_on("assembly", "definition"));
}

#[test]
fn representation_features_are_features_of_the_definitions_scope() {
    const FRAGMENT: &str = "which is no feature of the definition's scope";
    let with = |key: &str| {
        let mut ir = with_simplified();
        definition_mut(&mut ir, "part").representations[0].features = vec![fixture::feature(key)];
        ir
    };
    assert!(found(&with("extrude"), FRAGMENT).is_empty());
    assert_eq!(
        found(&with("absent"), FRAGMENT),
        error_on("part", "definition")
    );
}

#[test]
fn representation_features_need_the_design_class() {
    let mut ir = CadIr::empty();
    let mut part = ProductDefinition::new(definition("part"), Storage::Embedded);
    let mut shown = representation("datums", RepresentationContent::Unread);
    shown.features.push(fixture::feature("plane"));
    part.representations.push(shown);
    ir.model.product_definitions.push(part);
    let violation = ir.set_scope(ContentScope::geometry()).unwrap_err();
    assert_eq!(
        violation.arena.map(crate::document::ArenaName::as_str),
        Some("product_definitions")
    );
    ir.model.product_definitions[0].representations[0]
        .features
        .clear();
    assert!(ir.set_scope(ContentScope::geometry()).is_ok());
}

#[test]
fn a_definition_is_generated_from_another_definition() {
    const FRAGMENT: &str = "generated from itself";
    let with = |factory: &str| {
        let mut ir = assembly();
        definition_mut(&mut ir, "sub").generated_from = Some(GeneratedFrom {
            definition: Some(definition(factory)),
            configuration: ConfigurationRef::Named(name("base")),
        });
        ir
    };
    assert!(found(&with("part"), FRAGMENT).is_empty());
    assert_eq!(found(&with("sub"), FRAGMENT), error_on("sub", "definition"));
}

#[test]
fn a_generated_members_configuration_resolves_among_the_factorys() {
    const FRAGMENT: &str = "generated from no configuration of";
    let with = |configuration: ConfigurationRef| {
        let mut ir = assembly();
        definition_mut(&mut ir, "sub").generated_from = Some(GeneratedFrom {
            definition: Some(definition("part")),
            configuration,
        });
        ir
    };
    assert!(found(
        &with(ConfigurationRef::Local(id("configuration", "large"))),
        FRAGMENT
    )
    .is_empty());
    assert!(found(&with(ConfigurationRef::Named(name("large"))), FRAGMENT).is_empty());
    assert_eq!(
        found(&with(ConfigurationRef::Named(name("small"))), FRAGMENT),
        error_on("sub", "definition")
    );
    assert_eq!(
        found(
            &with(ConfigurationRef::Local(id("configuration", "small"))),
            FRAGMENT
        ),
        error_on("sub", "definition")
    );
}

#[test]
fn a_placement_with_a_singular_linear_part_is_a_warning() {
    const FRAGMENT: &str = "a placement's linear part is singular";
    let mut ir = assembly();
    occurrence_mut(&mut ir, "b").transform = Some(Transform::identity());
    assert!(found(&ir, FRAGMENT).is_empty());
    occurrence_mut(&mut ir, "b").transform = Transform::affine([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0],
    ]);
    assert_eq!(
        found(&ir, FRAGMENT),
        [(Severity::Warning, Some(occurrence("b").as_str().to_owned()))]
    );
}

/// A nested override's path resolves from the overridden occurrence's
/// prototype and states no step configuration (§5.8, §5.11).
#[test]
fn a_nested_override_path_resolves_from_the_prototype_unconfigured() {
    let nested = |keys: &[&str]| NestedOverride {
        path: path(keys),
        transform: None,
        suppressed: None,
        visible: None,
        representation: None,
    };
    let mut ir = assembly();
    occurrence_mut(&mut ir, "a").overrides.push(nested(&["b"]));
    assert_eq!(found(&ir, "occurrence path"), Vec::new());
    assert_eq!(found(&ir, "step configuration"), Vec::new());

    let mut ir = assembly();
    occurrence_mut(&mut ir, "a").overrides.push(nested(&["a"]));
    assert_eq!(
        found(&ir, "is not an occurrence of the previous step's prototype"),
        error_on("a", "occurrence")
    );

    let mut ir = assembly();
    let mut configured = nested(&["b"]);
    configured.path.0[0].configuration = Some(ConfigurationRef::Local(id("configuration", "base")));
    occurrence_mut(&mut ir, "a").overrides.push(configured);
    assert_eq!(
        found(&ir, "a nested override's path states a step configuration"),
        error_on("a", "occurrence")
    );
}

/// An instance-body path and a representation member's path state no step
/// configuration (§5.8).
#[test]
fn product_paths_state_no_step_configuration() {
    let mut ir = assembly();
    let body = fixture::body(&ir);
    let mut entry = path(&["a", "b"]);
    entry.0[1].configuration = Some(ConfigurationRef::Local(id("configuration", "base")));
    definition_mut(&mut ir, "assembly")
        .instance_bodies
        .push(InstanceBodies {
            occurrence: entry,
            bodies: vec![body],
        });
    assert_eq!(
        found(&ir, "an instance-body path states a step configuration"),
        error_on("assembly", "definition")
    );
}

#[test]
fn a_definition_unit_system_names_units_of_their_quantities() {
    use crate::design::units::{UnitSymbol, UnitSystem};
    let mut ir = fixture::document();
    definition_mut(&mut ir, "part").units = Some(UnitSystem {
        length: UnitSymbol::In,
        angle: UnitSymbol::Deg,
        mass: Some(UnitSymbol::Lb),
        time: Some(UnitSymbol::Min),
    });
    assert!(found(&ir, "unit `").is_empty());

    definition_mut(&mut ir, "part").units = Some(UnitSystem {
        length: UnitSymbol::Deg,
        angle: UnitSymbol::Mm,
        mass: Some(UnitSymbol::S),
        time: Some(UnitSymbol::Kg),
    });
    for (what, unit) in [
        ("length", "deg"),
        ("angle", "mm"),
        ("mass", "s"),
        ("time", "kg"),
    ] {
        assert_eq!(
            found(
                &ir,
                &format!("the definition's {what} unit `{unit}` is no {what} unit")
            ),
            error_on("part", "definition"),
        );
    }
}
