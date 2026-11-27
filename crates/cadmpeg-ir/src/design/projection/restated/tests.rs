// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::text::NonBlankString;

use super::same_id_cuts;
use crate::design::construction::{Construction, FrameValue};
use crate::design::evaluation::EvaluationStatus;
use crate::design::feature::{Feature, FeatureDefinition, FeatureOperation, OperationFamily};
use crate::design::ops::sketch::{
    DefiningRole, EntityDefinition, EntityRole, Planar, PlanarCurve, Sketch, SketchEntity,
    SketchPlane, SketchSolve,
};
use crate::design::options::NativeOptions;
use crate::design::parameter::{
    DesignParameter, ParameterDefinition, ParameterOwner, ParameterValue,
};
use crate::design::units::{Quantity, ValueKind};
use crate::features::parameters::{
    DesignParameter as ModelParameter, ParameterValue as ModelValue,
};
use crate::features::{FeatureId, ParameterId};
use crate::ids::ProductDefinitionId;
use crate::math::{Point2, Point3, Vector3};
use crate::scalar::{FiniteReal, Length};
use crate::sketches::{
    Sketch as ModelSketch, SketchEntity as ModelEntity, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchPlacement, SketchProfiles,
};
use crate::units::UnitVector3;
use crate::CadIr;

const SKETCH: &str = "test:design:sketch#base";
const LINE: &str = "test:design:entity#line";
const START: &str = "test:design:entity#start";
const END: &str = "test:design:entity#end";

/// The cuts of every record of `ir`, each written.
fn cuts(ir: &CadIr) -> BTreeMap<String, BTreeSet<String>> {
    let written = ir
        .model
        .sketches
        .iter()
        .map(|sketch| sketch.id.as_str())
        .chain(
            ir.model
                .sketch_entities
                .iter()
                .map(|entity| entity.id().as_str()),
        )
        .chain(
            ir.model
                .parameters
                .iter()
                .map(|parameter| parameter.id.as_str()),
        )
        .collect::<HashSet<_>>();
    same_id_cuts(ir, &written)
        .into_iter()
        .map(|(id, cut)| {
            (
                id.to_owned(),
                cut.into_iter().map(std::borrow::Cow::into_owned).collect(),
            )
        })
        .collect()
}

fn fields<const N: usize>(fields: [&str; N]) -> BTreeSet<String> {
    fields.into_iter().map(str::to_owned).collect()
}

fn sketch_id() -> SketchId {
    SketchId::mint(SKETCH).unwrap()
}

fn entity_id(key: &str) -> SketchEntityId {
    SketchEntityId::mint(key).unwrap()
}

fn geometry(raw: SketchGeometryDefinition) -> SketchGeometry {
    raw.try_into().unwrap()
}

/// A model sketch on a resolved frame tilted out of the principal planes,
/// holding a construction line from (0, 0) to (3, 4) and its two endpoint
/// points, each with a native record; and the design records a projection
/// writes for them.
fn sketch_fixture() -> CadIr {
    let mut ir = CadIr::empty();
    let placement = SketchPlacement::try_resolved(
        Point3::new(1.0, 2.0, 3.0),
        Vector3::new(0.0, 0.0, 2.0),
        Vector3::new(3.0, 4.0, 0.0),
    )
    .unwrap();
    ir.model.sketches.push(ModelSketch {
        id: sketch_id(),
        name: Some("Profile".to_owned()),
        configuration: None,
        visible: None,
        placement,
        profiles: SketchProfiles::default(),
        native_ref: Some("test:native#sketch".to_owned()),
    });
    let point = |key: &str, native: &str, u: f64, v: f64| {
        ModelEntity::new(
            entity_id(key),
            sketch_id(),
            geometry(SketchGeometryDefinition::Point {
                position: Point2::new(u, v),
            }),
        )
        .with_native_ref(Some(native.to_owned()))
    };
    ir.model.sketch_entities.extend([
        ModelEntity::new(
            entity_id(LINE),
            sketch_id(),
            geometry(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(3.0, 4.0),
            }),
        )
        .with_construction(true)
        .with_native_ref(Some("test:native#line".to_owned()))
        .with_endpoint_refs(vec![
            "test:native#start".to_owned(),
            "test:native#end".to_owned(),
        ]),
        point(START, "test:native#start", 0.0, 0.0),
        point(END, "test:native#end", 3.0, 4.0),
    ]);
    let (origin, normal, u_axis) = placement.resolved().unwrap();
    let z_axis = UnitVector3::normalized_nonzero(normal).unwrap();
    let x_axis = UnitVector3::normalized_nonzero(u_axis).unwrap();
    ir.design.sketches.push(Sketch {
        id: sketch_id(),
        owner: FeatureId::mint("test:design:feature#sketch").unwrap(),
        name: Some("Profile".to_owned()),
        plane: SketchPlane::Placed(Box::new(Construction {
            inputs: Vec::new(),
            after: Vec::new(),
            solution: None,
            mode: None,
            resolved: Some(FrameValue {
                origin,
                x_axis,
                y_axis: UnitVector3::normalized(z_axis.as_raw().cross(*x_axis.as_raw())).unwrap(),
                z_axis,
            }),
            resolved_in: BTreeMap::default(),
        })),
        solve: SketchSolve {
            state: None,
            status: EvaluationStatus::Current,
        },
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
        native_ref: Some("test:native#sketch".to_owned()),
    });
    let entities = ir
        .model
        .sketch_entities
        .iter()
        .map(|model| {
            let curve = match model.geometry.definition() {
                SketchGeometryDefinition::Point { position } => PlanarCurve::Point(*position),
                SketchGeometryDefinition::Line { start, end } => PlanarCurve::Line {
                    start: *start,
                    end: *end,
                },
                other => panic!("{other:?}"),
            };
            let role = match model.id().as_str() {
                START => Some(DefiningRole::Start),
                END => Some(DefiningRole::End),
                _ => None,
            };
            SketchEntity::<Planar> {
                id: model.id().clone(),
                sketch: sketch_id(),
                definition: EntityDefinition::Free(curve),
                roles: role
                    .map(|role| EntityRole {
                        of: entity_id(LINE),
                        role,
                    })
                    .into_iter()
                    .collect(),
                construction: Some(model.construction),
                centerline: None,
                native: None,
                native_options: NativeOptions::new(),
                source_settings: NativeOptions::new(),
                native_ref: model.native_ref.clone(),
            }
        })
        .collect::<Vec<_>>();
    ir.design.sketch_entities.extend(entities);
    ir
}

/// A design sketch carries the name, frame and native record of the model
/// sketch, and a design entity the curve, construction flag and endpoint
/// records of the model entity, under their own names and forms.
#[test]
fn a_projected_sketch_carries_the_decoded_sketch_and_entities() {
    let cuts = cuts(&sketch_fixture());
    for id in [SKETCH, LINE, START, END] {
        assert_eq!(cuts[id], fields([]), "{id}");
    }
}

/// A design entity names each field it does not state: a flag it marks
/// unread, a curve it moves, and an endpoint record no design point bounds
/// it with.
#[test]
fn a_projected_entity_names_what_it_does_not_state() {
    let mut ir = sketch_fixture();
    let line = ir
        .design
        .sketch_entities
        .iter_mut()
        .find(|entity| entity.id.as_str() == LINE)
        .unwrap();
    line.construction = None;
    let EntityDefinition::Free(PlanarCurve::Line { end, .. }) = &mut line.definition else {
        panic!("a free line");
    };
    *end = crate::units::FinitePoint2::try_from(Point2::new(3.0, 5.0)).unwrap();
    let end_point = ir
        .design
        .sketch_entities
        .iter_mut()
        .find(|entity| entity.id.as_str() == END)
        .unwrap();
    end_point.roles.clear();
    assert_eq!(
        cuts(&ir)[LINE],
        fields(["construction", "endpoint_refs", "geometry"])
    );
}

/// A design sketch on another frame names its placement.
#[test]
fn a_projected_sketch_on_another_frame_names_its_placement() {
    let mut ir = sketch_fixture();
    let SketchPlane::Placed(plane) = &mut ir.design.sketches[0].plane else {
        panic!("a placed sketch");
    };
    let frame = plane.resolved.as_mut().unwrap();
    std::mem::swap(&mut frame.x_axis, &mut frame.y_axis);
    assert_eq!(cuts(&ir)[SKETCH], fields(["placement"]));
}

const SCOPE: &str = "test:design:product-definition#root";
const COMPONENT: &str = "test:design:product-definition#component";

fn parameter_id(key: &str) -> ParameterId {
    ParameterId::mint(format!("test:design:parameter#{key}")).unwrap()
}

/// A model parameter `key` at `ordinal`, of 12 mm.
fn model_parameter(key: &str, ordinal: u32) -> ModelParameter {
    ModelParameter {
        product_definition: None,
        id: parameter_id(key),
        owner: None,
        ordinal,
        name: key.to_owned(),
        expression: String::new(),
        display: None,
        value: Some(ModelValue::Length(Length::new(12.0).unwrap())),
        dependencies: crate::features::selections::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    }
}

/// The design parameter `key` of 12 mm, named `name`.
fn design_parameter(key: &str, name: &str) -> DesignParameter {
    DesignParameter {
        id: parameter_id(key),
        scope: ProductDefinitionId::mint(SCOPE).unwrap(),
        owner: None,
        name: NonBlankString::new(name.to_owned()),
        kind: ValueKind::Quantity(Quantity::LENGTH),
        unit: None,
        definition: ParameterDefinition::Unset,
        value: Some(ParameterValue::Number(FiniteReal::new(12.0).unwrap())),
        display: None,
        tolerance: None,
        decimals: None,
        comment: None,
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
        native_ref: None,
    }
}

fn parameter_ir(models: Vec<ModelParameter>, designs: Vec<DesignParameter>) -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.parameters = models;
    ir.design.parameters = designs;
    ir
}

/// The design parameter arena holds the order of the numbers the ids carry:
/// a parameter whose ordinal follows them keeps its ordinal, one whose
/// ordinal reverses them names it.
#[test]
fn a_parameter_ordinal_follows_the_numbers_of_the_ids() {
    let designs = || {
        vec![
            design_parameter("sketch:10", "sketch:10"),
            design_parameter("sketch:7", "sketch:7"),
        ]
    };
    let ordered = cuts(&parameter_ir(
        vec![
            model_parameter("sketch:7", 0),
            model_parameter("sketch:10", 1),
        ],
        designs(),
    ));
    assert_eq!(ordered[parameter_id("sketch:7").as_str()], fields([]));
    assert_eq!(ordered[parameter_id("sketch:10").as_str()], fields([]));
    let reversed = cuts(&parameter_ir(
        vec![
            model_parameter("sketch:7", 1),
            model_parameter("sketch:10", 0),
        ],
        designs(),
    ));
    assert_eq!(
        reversed[parameter_id("sketch:7").as_str()],
        fields(["ordinal"])
    );
    assert_eq!(
        reversed[parameter_id("sketch:10").as_str()],
        fields(["ordinal"])
    );
}

/// A design parameter named by the stored name the model keeps in
/// `source_name` carries the model name the decoder generated; another name
/// names it.
#[test]
fn a_parameter_name_is_carried_by_its_stored_source_name() {
    let mut model = model_parameter("constraint:3", 0);
    model.name = "Constraint3".to_owned();
    model.properties.insert(
        NonBlankString::new("source_name".to_owned()).unwrap(),
        "Width".to_owned(),
    );
    let id = model.id.as_str().to_owned();
    let carried = cuts(&parameter_ir(
        vec![model.clone()],
        vec![design_parameter("constraint:3", "Width")],
    ));
    assert_eq!(carried[&id], fields(["properties"]));
    let renamed = cuts(&parameter_ir(
        vec![model],
        vec![design_parameter("constraint:3", "Depth")],
    ));
    assert_eq!(renamed[&id], fields(["name", "properties"]));
}

/// A design parameter owned by a feature the design lists under a component
/// carries the model definition of that component; a feature listed under
/// no component does not.
#[test]
fn a_parameter_definition_is_carried_by_its_owner_feature_component() {
    let owner = FeatureId::mint("test:design:feature#extrude").unwrap();
    let mut model = model_parameter("depth", 0);
    model.product_definition = Some(ProductDefinitionId::mint(COMPONENT).unwrap());
    model.owner = Some(owner.clone());
    let mut design = design_parameter("depth", "depth");
    design.owner = Some(ParameterOwner::Feature(owner.clone()));
    let feature = |component: Option<&str>| Feature {
        id: owner.clone(),
        name: None,
        suppressed: None,
        definition: FeatureDefinition {
            operation: FeatureOperation::Unresolved {
                family: OperationFamily::Extrude,
            },
            assembly: None,
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: component.map(|key| ProductDefinitionId::mint(key).unwrap()),
        driven_by: None,
        native_ref: None,
    };
    let id = model.id.as_str().to_owned();
    let mut ir = parameter_ir(vec![model], vec![design]);
    ir.design.features.push(feature(Some(COMPONENT)));
    assert_eq!(cuts(&ir)[&id], fields([]));
    ir.design.features[0] = feature(None);
    assert_eq!(cuts(&ir)[&id], fields(["product_definition"]));
}
