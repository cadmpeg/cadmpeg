// SPDX-License-Identifier: Apache-2.0
//! Tests of the planar and spatial sketches of a Fusion design.
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::design::construction::InputReference;
use cadmpeg_ir::design::construction::ScalarDimension;
use cadmpeg_ir::design::container::{Container, OrderedKind, UnorderedKind};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::ops::sketch::{
    DefiningRole, DistanceAxis, EntityDefinition, EntityForm, EntityRole, Operand, RadialMeasure,
    SketchPlane, SketchRelation,
};
use cadmpeg_ir::design::parameter::{ParameterDefinition, ParameterOwner, ParameterValue};
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::design::selection::Anchor;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::parameters::ParameterValue as V10;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{
    Feature as LegacyFeature, FeatureContent, FeatureDefinition as LegacyDefinition, FeatureId,
    FeatureOperation as LegacyOperation, ParameterId, SketchFeatureBinding,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::SketchId;

use crate::loss::F3dLossCode;

const STREAM: &str = "f3d:FusionAssetName[Active]/Design1/BulkStream.dat";
const OWNER: &str = "f3d:model:feature#sketch:20";
const SKETCH: &str = "f3d:model:sketch#20";

fn entity(key: &str) -> String {
    format!("f3d:model:sketch-entity#20:{key}")
}

fn constraint(key: &str) -> String {
    format!("f3d:model:sketch-constraint#20:{key}")
}

fn parameter_id(key: &str) -> String {
    format!("f3d:model:parameter#{key}")
}

/// The v10 feature `id` whose operation is the planar sketch `sketch`.
fn sketch_feature(id: &str, sketch: &str) -> LegacyFeature {
    LegacyFeature {
        product_definition: None,
        id: FeatureId::mint(id).unwrap(),
        ordinal: 0,
        name: Some("Sketch1".to_owned()),
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(LegacyDefinition::Operation(
            LegacyOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(SketchId::mint(sketch).unwrap())),
            },
        )),
        native_ref: Some(format!("{STREAM}:design-parameter-scope#20")),
    }
}

/// A length parameter `key` of the sketch feature that the source lists
/// under a linear dimension.
fn length(key: &str, value: f64) -> cadmpeg_ir::features::parameters::DesignParameter {
    cadmpeg_ir::features::parameters::DesignParameter {
        product_definition: None,
        id: ParameterId::mint(parameter_id(key)).unwrap(),
        owner: Some(FeatureId::mint(OWNER).unwrap()),
        ordinal: 0,
        name: key.to_owned(),
        expression: format!("{value} mm"),
        display: None,
        value: Some(V10::Length(Length::new(value).unwrap())),
        dependencies: DistinctMembers::default(),
        properties: [(
            cadmpeg_core::text::NonBlankString::new("source_kind".to_owned()).unwrap(),
            "Linear Dimension-3".to_owned(),
        )]
        .into_iter()
        .collect(),
        pmi: None,
        native_ref: Some(format!("{STREAM}:design-parameter#{key}")),
    }
}

/// A sketch on the XY plane with two points 30 mm apart along `u`, a circle
/// of radius 5 mm, and a text entity, constrained by a horizontal distance
/// bound to `width`, a radius bound to `radius`, a distance bound to
/// `absent`, which no record stores, and a horizontal relation.
fn model() -> Model {
    let mut model = Model::default();
    model.features = vec![sketch_feature(OWNER, SKETCH)];
    model.parameters = vec![length("width", 30.0), length("radius", 5.0)];
    model.sketches = json::from_value(cadmpeg_core::json!([{
        "id": SKETCH, "name": "Sketch1",
        "placement": {"kind": "resolved", "frame": {"origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0}, "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}}},
        "profiles": []
    }]))
    .unwrap();
    let point = |key: &str, u: f64| {
        cadmpeg_core::json!({"id": entity(key), "sketch": SKETCH, "construction": false,
            "endpoint_refs": [], "geometry": {"kind": "point", "position": {"u": u, "v": 0.0}}})
    };
    model.sketch_entities = json::from_value(cadmpeg_core::json!([
        point("p0", 0.0),
        point("p1", 30.0),
        {"id": entity("line"), "sketch": SKETCH, "construction": false, "endpoint_refs": [],
            "geometry": {"kind": "line", "start": {"u": 0.0, "v": 0.0}, "end": {"u": 30.0, "v": 0.0}}},
        {"id": entity("circle"), "sketch": SKETCH, "construction": true, "endpoint_refs": [],
            "geometry": {"kind": "circle", "center": {"u": 15.0, "v": 10.0}, "radius": 5.0}},
        {"id": entity("text"), "sketch": SKETCH, "construction": false, "endpoint_refs": [],
            "geometry": {"kind": "native", "native_kind": "SketchText"}}
    ]))
    .unwrap();
    model.sketch_constraints = json::from_value(cadmpeg_core::json!([
        {"id": constraint("width"), "sketch": SKETCH,
            "definition": {"kind": "horizontal_distance",
                "first": {"kind": "entity", "value": entity("p1")},
                "second": {"kind": "entity", "value": entity("p0")},
                "parameter": parameter_id("width")}},
        {"id": constraint("radius"), "sketch": SKETCH,
            "definition": {"kind": "radius", "entity": entity("circle"),
                "parameter": parameter_id("radius")}},
        {"id": constraint("absent"), "sketch": SKETCH,
            "definition": {"kind": "distance", "entities": [entity("p0"), entity("line")],
                "parameter": parameter_id("absent")}},
        {"id": constraint("horizontal"), "sketch": SKETCH,
            "definition": {"kind": "horizontal", "entity": entity("line")}}
    ]))
    .unwrap();
    model
}

fn unresolved_sketch(id: &str) -> Feature {
    Feature {
        id: FeatureId::mint(id).unwrap(),
        name: Some("Sketch1".to_owned()),
        suppressed: None,
        definition: FeatureDefinition {
            operation: FeatureOperation::Unresolved {
                family: OperationFamily::Sketch,
            },
            assembly: None,
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: None,
    }
}

static ROOT: std::sync::LazyLock<cadmpeg_ir::ids::ProductDefinitionId> =
    std::sync::LazyLock::new(crate::tree::file_root_id);

static NO_INCIDENCE: std::sync::LazyLock<super::Incidence> =
    std::sync::LazyLock::new(super::Incidence::default);

/// The scope of the root definition, whose design parameters are
/// `parameters` and whose unordered members are `unordered`.
fn scope<'a>(
    model: &'a Model,
    unordered: &'a BTreeSet<FeatureId>,
    parameters: &'a [&'a cadmpeg_ir::design::parameter::DesignParameter],
) -> super::Scope<'a> {
    scope_with(model, &NO_INCIDENCE, unordered, parameters)
}

/// The scope of [`scope`] whose curve records name the point records
/// `incidence` gives.
fn scope_with<'a>(
    model: &'a Model,
    incidence: &'a super::Incidence,
    unordered: &'a BTreeSet<FeatureId>,
    parameters: &'a [&'a cadmpeg_ir::design::parameter::DesignParameter],
) -> super::Scope<'a> {
    super::Scope {
        ctx: Box::leak(Box::new(
            cadmpeg_core::decode::test_support::service_decode_context(),
        )),
        model,
        incidence,
        linear: 1.0e-6,
        definition: &ROOT,
        unordered,
        parameters,
    }
}

fn names(losses: &[LossNote], code: F3dLossCode, record: &str) -> bool {
    losses
        .iter()
        .any(|loss| loss.code == code.kind() && loss.message.contains(record))
}

#[test]
fn a_sketch_whose_owner_has_no_timeline_item_is_owned_by_its_v10_feature_or_a_minted_one() {
    let mut model = model();
    let mut loose = model.sketches[0].clone();
    loose.id = SketchId::mint("f3d:model:sketch#21").unwrap();
    loose.name = Some("Loose".to_owned());
    model.sketches.push(loose);

    let outside =
        super::outside(&model, &BTreeSet::new(), &BTreeSet::new(), &mut Vec::new()).unwrap();
    let owners = outside.get("").unwrap();
    let [owned, minted] = owners.as_slice() else {
        panic!("two owners: {owners:#?}");
    };
    assert_eq!(owned.id.as_str(), OWNER);
    assert_eq!(
        owned.definition.operation,
        FeatureOperation::Sketch(SketchId::mint(SKETCH).unwrap())
    );
    assert!(minted.id.as_str().starts_with("f3d:design:sketch-owner#"));
    assert_eq!(minted.name.as_deref(), Some("Loose"));
    assert_eq!(
        minted.definition.operation,
        FeatureOperation::Sketch(SketchId::mint("f3d:model:sketch#21").unwrap())
    );

    let timeline = BTreeSet::from([FeatureId::mint(OWNER).unwrap()]);
    let outside = super::outside(&model, &timeline, &BTreeSet::new(), &mut Vec::new()).unwrap();
    let [minted] = outside.get("").unwrap().as_slice() else {
        panic!("one owner: {outside:#?}");
    };
    assert_eq!(minted.name.as_deref(), Some("Loose"));
}

#[test]
fn the_group_and_a_minted_owner_of_an_archive_member_take_the_member_key() {
    assert_eq!(
        super::group_id("").unwrap().as_str(),
        "f3d:design:container#sketches"
    );
    assert_eq!(
        super::group_id("member.f3d@").unwrap().as_str(),
        "f3d:member.f3d@design:container#sketches"
    );

    let mut model = model();
    model.features.clear();
    model.sketches[0].id = SketchId::mint("f3d:member.f3d@model:sketch#21").unwrap();
    let outside =
        super::outside(&model, &BTreeSet::new(), &BTreeSet::new(), &mut Vec::new()).unwrap();
    let [minted] = outside.values().flatten().collect::<Vec<_>>()[..] else {
        panic!("one owner: {outside:#?}");
    };
    assert!(minted
        .id
        .as_str()
        .starts_with("f3d:member.f3d@design:sketch-owner#"));
}

#[test]
fn a_group_beside_a_timeline_and_a_group_alone_are_roots_that_hold_their_members() {
    const GROUP: &str = "f3d:design:container#sketches";
    let hold = |into: &mut super::timeline::Projected| {
        crate::test_support::with_decode_context(|ctx| {
            super::hold(ctx, "", vec![unresolved_sketch(OWNER)], into)
        })
        .unwrap();
    };
    let group = |into: &super::timeline::Projected| {
        into.containers
            .iter()
            .find_map(|container| match container {
                Container::Unordered { id, kind, members } if id.as_str() == GROUP => {
                    Some((*kind, members.clone()))
                }
                _ => None,
            })
    };
    let owner = Member::Feature(FeatureId::mint(OWNER).unwrap());

    let mut alone = super::timeline::Projected::default();
    hold(&mut alone);
    assert_eq!(
        group(&alone),
        Some((UnorderedKind::Group, BTreeSet::from([owner.clone()])))
    );
    assert_eq!(alone.features.len(), 1);

    let mut beside = super::timeline::Projected::default();
    beside.containers.push(Container::Ordered {
        id: ContainerId::mint("f3d:design:container#timeline").unwrap(),
        kind: OrderedKind::Timeline,
        members: Vec::new(),
        folders: Vec::new(),
        rollback: None,
        placement: None,
    });
    hold(&mut beside);
    assert_eq!(
        group(&beside),
        Some((UnorderedKind::Group, BTreeSet::from([owner])))
    );
    assert_eq!(beside.containers.len(), 2);
    assert!(matches!(beside.containers[0], Container::Ordered { .. }));
}

#[test]
fn a_sketch_states_its_entities_and_binds_its_dimensions_to_parameters_of_its_scope() {
    let model = model();
    let definitions = BTreeMap::from([(String::new(), crate::tree::file_root_id())]);
    let mut losses = Vec::new();
    let parameters = super::super::parameters::project(
        &model.parameters,
        &definitions,
        &BTreeSet::new(),
        &BTreeSet::from([OWNER]),
        &mut losses,
    );
    let in_scope = parameters
        .iter()
        .filter(|parameter| parameter.id.as_str() == parameter_id("width"))
        .collect::<Vec<_>>();
    assert_eq!(in_scope.len(), 1, "{parameters:#?}");

    let mut features = [unresolved_sketch(OWNER)];
    let mut out = super::Sketches::default();
    super::project(
        &scope(&model, &BTreeSet::new(), &in_scope),
        &mut features,
        &mut out,
        &mut losses,
    )
    .expect("the session admits the evaluation");

    let sketch = SketchId::mint(SKETCH).unwrap();
    assert_eq!(
        features[0].definition.operation,
        FeatureOperation::Sketch(sketch.clone())
    );
    let [record] = out.records.as_slice() else {
        panic!("one sketch: {:#?}", out.records);
    };
    assert_eq!(record.owner.as_str(), OWNER);
    let SketchPlane::Placed(plane) = &record.plane else {
        panic!("a placed plane");
    };
    let Some(InputReference::Picks(selection)) = &plane.inputs[0].reference else {
        panic!("an unread input");
    };
    assert_eq!(selection.at, Anchor::Input);
    assert!(plane.resolved.is_some());

    assert_eq!(out.entities.len(), 5);
    let text = out
        .entities
        .iter()
        .find(|candidate| candidate.id.as_str() == entity("text"))
        .unwrap();
    let EntityDefinition::Defined { form, solved: None } = &text.definition else {
        panic!("a native entity: {:#?}", text.definition);
    };
    assert!(matches!(form.as_ref(), EntityForm::Native(kind) if kind.as_str() == "f3d:SketchText"));
    assert!(names(
        &losses,
        F3dLossCode::DesignSketchEntityNative,
        &entity("text")
    ));
    let circle = out
        .entities
        .iter()
        .find(|candidate| candidate.id.as_str() == entity("circle"))
        .unwrap();
    assert_eq!(circle.construction, Some(true));

    let relation = |key: &str| {
        &out.constraints
            .iter()
            .find(|candidate| candidate.id.as_str() == constraint(key))
            .unwrap()
            .relation
    };
    let point = |key: &str| {
        Operand::Entity(cadmpeg_ir::sketches::SketchEntityId::mint(entity(key)).unwrap())
    };
    assert_eq!(
        relation("width"),
        &SketchRelation::Distance {
            a: point("p0"),
            b: point("p1"),
            along: Some(DistanceAxis::Horizontal),
            doubled: None,
            value: Dimension::Parameter(ParameterId::mint(parameter_id("width")).unwrap()),
        }
    );
    assert!(!names(
        &losses,
        F3dLossCode::DesignSketchDimensionUnbound,
        &constraint("width")
    ));
    let SketchRelation::Radial {
        measure: RadialMeasure::Radius,
        value: Dimension::Literal(radius),
        ..
    } = relation("radius")
    else {
        panic!("a radius of the stored value: {:#?}", relation("radius"));
    };
    assert_eq!(radius.get(), 5.0);
    assert!(names(
        &losses,
        F3dLossCode::DesignSketchDimensionUnbound,
        &constraint("radius")
    ));
    assert!(
        matches!(relation("absent"), SketchRelation::Native { kind, values, .. }
            if kind.as_str() == "f3d:distance" && values.is_empty()),
        "{:#?}",
        relation("absent")
    );
    assert!(names(
        &losses,
        F3dLossCode::DesignSketchDimensionUnbound,
        &constraint("absent")
    ));
    assert!(matches!(
        relation("horizontal"),
        SketchRelation::Horizontal { b: None, .. }
    ));
}

#[test]
fn a_sketch_of_an_unordered_member_anchors_its_plane_in_the_current_state() {
    let model = model();
    let mut features = [unresolved_sketch(OWNER)];
    let mut out = super::Sketches::default();
    let unordered = BTreeSet::from([FeatureId::mint(OWNER).unwrap()]);
    super::project(
        &scope(&model, &unordered, &[]),
        &mut features,
        &mut out,
        &mut Vec::new(),
    )
    .expect("the session admits the evaluation");
    let SketchPlane::Placed(plane) = &out.records[0].plane else {
        panic!("a placed plane");
    };
    let Some(InputReference::Picks(selection)) = &plane.inputs[0].reference else {
        panic!("an unread input");
    };
    assert_eq!(selection.at, Anchor::Current);
}

/// The design parameters of the scope of `model` whose ids are `keys`.
fn in_scope(model: &Model, keys: &[&str]) -> Vec<cadmpeg_ir::design::parameter::DesignParameter> {
    let definitions = BTreeMap::from([(String::new(), crate::tree::file_root_id())]);
    super::super::parameters::project(
        &model.parameters,
        &definitions,
        &BTreeSet::new(),
        &BTreeSet::from([OWNER]),
        &mut Vec::new(),
    )
    .into_iter()
    .filter(|parameter| {
        keys.iter()
            .any(|key| parameter.id.as_str() == parameter_id(key))
    })
    .collect()
}

#[test]
fn a_native_dimension_binds_its_parameter_and_a_reference_dimension_binds_a_driven_one() {
    let mut model = model();
    model.parameters.push(length("gap", 7.0));
    let mut constraints = json::to_value(&model.sketch_constraints).unwrap();
    let list = constraints.as_array_mut().unwrap();
    list.push(
        cadmpeg_core::json!({"id": constraint("native"), "sketch": SKETCH,
        "definition": {"kind": "native", "native_kind": "Linear Dimension",
            "native_properties": {}, "entities": [entity("line")],
            "parameter": parameter_id("width"), "operands": []}}),
    );
    list.push(
        cadmpeg_core::json!({"id": constraint("gap"), "sketch": SKETCH, "driving": false,
        "definition": {"kind": "distance", "entities": [entity("p0"), entity("p1")],
            "parameter": parameter_id("gap")}}),
    );
    model.sketch_constraints = json::from_value(constraints).unwrap();
    let parameters = in_scope(&model, &["width"]);
    let parameters = parameters.iter().collect::<Vec<_>>();
    let mut features = [unresolved_sketch(OWNER)];
    let mut out = super::Sketches::default();
    let mut losses = Vec::new();
    super::project(
        &scope(&model, &BTreeSet::new(), &parameters),
        &mut features,
        &mut out,
        &mut losses,
    )
    .expect("the session admits the evaluation");
    let relation = |key: &str| {
        &out.constraints
            .iter()
            .find(|candidate| candidate.id.as_str() == constraint(key))
            .unwrap()
            .relation
    };
    let width = ParameterId::mint(parameter_id("width")).unwrap();
    let SketchRelation::Native { kind, values, .. } = relation("native") else {
        panic!("a native dimension: {:#?}", relation("native"));
    };
    assert_eq!(kind.as_str(), "f3d:Linear Dimension");
    assert_eq!(
        values,
        &vec![ScalarDimension::Length(Dimension::Parameter(width))]
    );
    assert!(!names(
        &losses,
        F3dLossCode::DesignSketchDimensionUnbound,
        &constraint("native")
    ));

    let gap = ParameterId::mint(parameter_id("gap")).unwrap();
    let SketchRelation::Distance { value, .. } = relation("gap") else {
        panic!("a distance: {:#?}", relation("gap"));
    };
    assert_eq!(value, &Dimension::Parameter(gap.clone()));
    let [minted] = out.parameters.as_slice() else {
        panic!("one minted parameter: {:#?}", out.parameters);
    };
    assert_eq!(minted.id, gap);
    assert_eq!(minted.definition, ParameterDefinition::Driven);
    assert_eq!(
        minted
            .name
            .as_ref()
            .map(cadmpeg_core::text::NonBlankString::as_str),
        Some("gap")
    );
    assert_eq!(
        minted.owner,
        Some(ParameterOwner::Feature(FeatureId::mint(OWNER).unwrap()))
    );
    assert!(matches!(
        minted.value,
        Some(ParameterValue::Number(value)) if value.get() == 7.0
    ));
    assert!(!names(
        &losses,
        F3dLossCode::DesignSketchDimensionUnbound,
        &constraint("gap")
    ));
}

#[test]
fn a_relation_on_a_line_end_names_a_point_minted_there() {
    let mut model = model();
    let mut entities = json::to_value(&model.sketch_entities).unwrap();
    entities.as_array_mut().unwrap()[2]["geometry"]["end"]["u"] = cadmpeg_core::json!(40.0);
    model.sketch_entities = json::from_value(entities).unwrap();
    let mut constraints = json::to_value(&model.sketch_constraints).unwrap();
    constraints
        .as_array_mut()
        .unwrap()
        .push(cadmpeg_core::json!({
        "id": constraint("on"), "sketch": SKETCH,
        "definition": {"kind": "point_on_object",
            "point": {"kind": "end", "value": entity("line")}, "entity": entity("circle")}}));
    model.sketch_constraints = json::from_value(constraints).unwrap();
    let mut features = [unresolved_sketch(OWNER)];
    let mut out = super::Sketches::default();
    super::project(
        &scope(&model, &BTreeSet::new(), &[]),
        &mut features,
        &mut out,
        &mut Vec::new(),
    )
    .expect("the session admits the evaluation");
    let end = format!("{}:end", entity("line"));
    let point = out
        .entities
        .iter()
        .find(|candidate| candidate.id.as_str() == end)
        .unwrap();
    assert_eq!(
        point.roles,
        vec![EntityRole {
            of: cadmpeg_ir::sketches::SketchEntityId::mint(entity("line")).unwrap(),
            role: DefiningRole::End,
        }]
    );
    let on = out
        .constraints
        .iter()
        .find(|candidate| candidate.id.as_str() == constraint("on"))
        .unwrap();
    assert!(
        matches!(&on.relation, SketchRelation::Coincident {
            a: Operand::Entity(a), ..
        } if a.as_str() == end),
        "{:#?}",
        on.relation
    );
}

#[test]
fn a_spatial_sketch_is_the_operation_of_its_feature_and_construction_joins_its_group() {
    const SPATIAL: &str = "f3d:model:spatial-sketch#20";
    let mut model = model();
    model.sketches.clear();
    model.sketch_entities.clear();
    model.sketch_constraints.clear();
    model.features[0].evaluation = FeatureEvaluation::from_definition(LegacyDefinition::Operation(
        LegacyOperation::SpatialSketch {
            sketch: Some(cadmpeg_ir::sketches::SpatialSketchId::mint(SPATIAL).unwrap()),
        },
    ));
    let mut point = sketch_feature("f3d:model:feature#point:21", SKETCH);
    point.evaluation = FeatureEvaluation::from_definition(LegacyDefinition::Operation(
        LegacyOperation::PointGeometry {
            position: cadmpeg_ir::features::geometry::FinitePoint3::new(
                cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            )
            .unwrap(),
        },
    ));
    model.features.push(point);
    model.spatial_sketches = json::from_value(cadmpeg_core::json!([{
        "id": SPATIAL, "name": "Path", "profiles": []
    }]))
    .unwrap();
    model.spatial_sketch_entities = json::from_value(cadmpeg_core::json!([{
        "id": "f3d:model:spatial-sketch-entity#20:line", "sketch": SPATIAL,
        "construction": false, "endpoint_refs": [],
        "geometry": {"kind": "line", "start": {"x": 0.0, "y": 0.0, "z": 0.0},
            "end": {"x": 0.0, "y": 0.0, "z": 2.0}}
    }, {
        "id": "f3d:model:spatial-sketch-entity#20:rise", "sketch": SPATIAL,
        "construction": false, "endpoint_refs": [],
        "geometry": {"kind": "line", "start": {"x": 0.0, "y": 0.0, "z": 2.0},
            "end": {"x": 1.0, "y": 0.0, "z": 3.0}}
    }]))
    .unwrap();
    model.spatial_sketch_constraints = json::from_value(cadmpeg_core::json!([{
        "id": constraint("group"), "sketch": SPATIAL,
        "definition": {"kind": "spline_group", "entities": [
            "f3d:model:spatial-sketch-entity#20:line", "f3d:model:spatial-sketch-entity#20:rise"]}
    }]))
    .unwrap();

    let mut losses = Vec::new();
    let outside = super::outside(&model, &BTreeSet::new(), &BTreeSet::new(), &mut losses).unwrap();
    let [owner, construction] = outside.get("").unwrap().as_slice() else {
        panic!("a sketch owner and a construction point: {outside:#?}");
    };
    assert_eq!(owner.id.as_str(), OWNER);
    assert_eq!(
        owner.definition.operation,
        FeatureOperation::SpatialSketch(
            cadmpeg_ir::sketches::SpatialSketchId::mint(SPATIAL).unwrap()
        )
    );
    assert_eq!(construction.id.as_str(), "f3d:model:feature#point:21");
    assert_eq!(
        construction.definition.operation,
        FeatureOperation::Unresolved {
            family: OperationFamily::Datum
        }
    );
    let listing = BTreeSet::from([String::new()]);
    let beside = super::outside(&model, &BTreeSet::new(), &listing, &mut losses).unwrap();
    assert_eq!(beside.get("").unwrap().len(), 1, "{beside:#?}");

    let mut features = [owner.clone()];
    let mut out = super::Sketches::default();
    super::project(
        &scope(&model, &BTreeSet::new(), &[]),
        &mut features,
        &mut out,
        &mut losses,
    )
    .expect("the session admits the evaluation");
    let [sketch] = out.spatial.as_slice() else {
        panic!("one spatial sketch: {:#?}", out.spatial);
    };
    assert_eq!(sketch.owner.as_str(), OWNER);
    assert_eq!(sketch.name.as_deref(), Some("Path"));
    assert_eq!(out.spatial_entities.len(), 2);
    assert!(out.records.is_empty());
    assert!(names(
        &losses,
        F3dLossCode::DesignSketchRelationNative,
        &constraint("group")
    ));
}

#[test]
fn a_relation_on_the_start_of_a_circle_states_the_circle_and_is_charged() {
    let mut model = model();
    let mut constraints = json::to_value(&model.sketch_constraints).unwrap();
    constraints
        .as_array_mut()
        .unwrap()
        .push(cadmpeg_core::json!({
        "id": constraint("on"), "sketch": SKETCH,
        "definition": {"kind": "point_on_object",
            "point": {"kind": "start", "value": entity("circle")}, "entity": entity("line")}}));
    model.sketch_constraints = json::from_value(constraints).unwrap();
    let mut features = [unresolved_sketch(OWNER)];
    let mut out = super::Sketches::default();
    let mut losses = Vec::new();
    super::project(
        &scope(&model, &BTreeSet::new(), &[]),
        &mut features,
        &mut out,
        &mut losses,
    )
    .expect("the session admits the evaluation");
    assert!(
        out.constraints
            .iter()
            .any(|candidate| candidate.id.as_str() == constraint("on")),
        "{:#?}",
        out.constraints
    );
    assert!(names(
        &losses,
        F3dLossCode::DesignSketchLocusUnplaced,
        &constraint("on")
    ));
}

/// The point records of [`model`] with native records, a construction arc
/// about `p0` from `p1`, and a point `stray` at `p0` that no curve names.
fn stored_points_model() -> (Model, super::Incidence) {
    let mut model = model();
    let mut entities = json::to_value(&model.sketch_entities).unwrap();
    let list = entities.as_array_mut().unwrap();
    for (index, record) in [(0, "point#1"), (1, "point#2"), (2, "curve#10")] {
        list[index]["native_ref"] = cadmpeg_core::json!(format!("{STREAM}:sketch-{record}"));
    }
    list.push(cadmpeg_core::json!({"id": entity("arc"), "sketch": SKETCH,
        "construction": true, "endpoint_refs": [],
        "native_ref": format!("{STREAM}:sketch-curve#11"),
        "geometry": {"kind": "arc", "center": {"u": 0.0, "v": 0.0}, "radius": 30.0,
            "start_angle": 0.0, "end_angle": 1.5}}));
    list.push(
        cadmpeg_core::json!({"id": entity("stray"), "sketch": SKETCH,
        "construction": false, "endpoint_refs": [],
        "native_ref": format!("{STREAM}:sketch-point#3"),
        "geometry": {"kind": "point", "position": {"u": 0.0, "v": 0.0}}}),
    );
    model.sketch_entities = json::from_value(entities).unwrap();
    let mut constraints = json::to_value(&model.sketch_constraints).unwrap();
    let list = constraints.as_array_mut().unwrap();
    for (key, locus, curve) in [("end", "end", "line"), ("center", "center", "arc")] {
        list.push(cadmpeg_core::json!({
            "id": constraint(key), "sketch": SKETCH,
            "definition": {"kind": "point_on_object",
                "point": {"kind": locus, "value": entity(curve)}, "entity": entity("circle")}}));
    }
    model.sketch_constraints = json::from_value(constraints).unwrap();
    let curve = |index: u32| super::CurveRecord {
        id: format!("{STREAM}:sketch-curve#{index}"),
        record_index: index,
    };
    let point = |index: u32, incident_curves: Vec<u32>| super::PointRecord {
        id: format!("{STREAM}:sketch-point#{index}"),
        companion: super::PointCompanion { incident_curves },
    };
    let incidence = super::Incidence::new(
        &[curve(10), curve(11)],
        &[
            point(1, vec![10, 11]),
            point(2, vec![10, 11]),
            point(3, Vec::new()),
        ],
    );
    (model, incidence)
}

#[test]
fn a_line_with_stored_end_points_and_an_arc_with_a_stored_center_name_those_points() {
    let (model, incidence) = stored_points_model();
    let mut features = [unresolved_sketch(OWNER)];
    let mut out = super::Sketches::default();
    let mut losses = Vec::new();
    super::project(
        &scope_with(&model, &incidence, &BTreeSet::new(), &[]),
        &mut features,
        &mut out,
        &mut losses,
    )
    .expect("the session admits the evaluation");
    let id = |key: &str| cadmpeg_ir::sketches::SketchEntityId::mint(entity(key)).unwrap();
    let roles = |key: &str| {
        out.entities
            .iter()
            .find(|candidate| candidate.id == id(key))
            .unwrap()
            .roles
            .clone()
    };
    let role = |of: &str, role| EntityRole { of: id(of), role };
    assert_eq!(
        roles("p0"),
        vec![
            role("arc", DefiningRole::Center),
            role("line", DefiningRole::Start)
        ]
    );
    assert_eq!(
        roles("p1"),
        vec![
            role("arc", DefiningRole::Start),
            role("line", DefiningRole::End)
        ]
    );
    assert!(roles("stray").is_empty());
    assert_eq!(
        out.entities.len(),
        7,
        "no point is minted: {:#?}",
        out.entities
    );
    let named = |key: &str| {
        let relation = &out
            .constraints
            .iter()
            .find(|candidate| candidate.id.as_str() == constraint(key))
            .unwrap()
            .relation;
        match relation {
            SketchRelation::Coincident {
                a: Operand::Entity(a),
                ..
            } => a.clone(),
            _ => panic!("a coincidence: {relation:#?}"),
        }
    };
    assert_eq!(named("end"), id("p1"));
    assert_eq!(named("center"), id("p0"));
    assert!(!names(
        &losses,
        F3dLossCode::DesignSketchLocusUnplaced,
        &constraint("center")
    ));
}

#[test]
fn two_points_at_a_center_that_no_point_record_names_leave_it_to_a_minted_point() {
    let (model, _) = stored_points_model();
    let mut features = [unresolved_sketch(OWNER)];
    let mut out = super::Sketches::default();
    super::project(
        &scope(&model, &BTreeSet::new(), &[]),
        &mut features,
        &mut out,
        &mut Vec::new(),
    )
    .expect("the session admits the evaluation");
    let center = format!("{}:center", entity("arc"));
    assert!(
        out.entities
            .iter()
            .any(|candidate| candidate.id.as_str() == center),
        "{:#?}",
        out.entities
    );
}

#[test]
fn a_document_with_no_timeline_item_and_no_sketch_holds_its_construction_features() {
    let mut model = model();
    model.sketches.clear();
    model.features[0].evaluation = FeatureEvaluation::from_definition(LegacyDefinition::Operation(
        LegacyOperation::PointGeometry {
            position: cadmpeg_ir::features::geometry::FinitePoint3::new(
                cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            )
            .unwrap(),
        },
    ));
    let outside =
        super::outside(&model, &BTreeSet::new(), &BTreeSet::new(), &mut Vec::new()).unwrap();
    let [construction] = outside.get("").unwrap().as_slice() else {
        panic!("one construction point: {outside:#?}");
    };
    assert_eq!(construction.id.as_str(), OWNER);
    let listing = BTreeSet::from([String::new()]);
    let listed = super::outside(&model, &BTreeSet::new(), &listing, &mut Vec::new()).unwrap();
    assert!(listed.is_empty(), "{listed:#?}");
}
