// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

use cadmpeg_core::decode::ContentScope;

use crate::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
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
            AttributeTarget::Tessellation(id("tessellation")),
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
        id: id.into(),
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
            id: "test:model:cloud#1".into(),
        }));
    assert_eq!(
        design_findings(&ir, BINDING),
        ["record target `test:model:cloud#1` is no record of arena `point_clouds`"]
    );
    ir.model.appearance_bindings[0] = binding(AppearanceTarget::Occurrence(
        "test:model:occurrence#1".try_into().unwrap(),
    ));
    assert_eq!(
        design_findings(&ir, BINDING),
        ["references missing occurrence `test:model:occurrence#1`"]
    );
}

#[test]
fn native_refs_of_appearances_bindings_and_attributes_resolve() {
    let mut ir = unit_cube().unwrap();
    ir.native.namespace_mut("native");
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
        ir.set_scope(ContentScope::geometry()).unwrap();
        ir.model.attributes.push(attribute(target, values));
        ir.check_scope().map_err(|violation| violation.to_string())
    };
    let violation = Err(
        "the document's scope excludes design but model arena `attributes` holds a reference into it"
            .to_owned(),
    );
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
    assert_eq!(scoped(AttributeTarget::Document, vec![None]), Ok(()));
}
