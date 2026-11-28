// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::text::NonBlankString;

use super::same_id_cuts;
use crate::design::construction::{Construction, FrameValue};
use crate::design::dimension::Dimension;
use crate::design::evaluation::EvaluationStatus;
use crate::design::feature::{Feature, FeatureDefinition, FeatureOperation, OperationFamily};
use crate::design::ops::sketch::{
    DefiningRole, EntityDefinition, EntityForm, EntityRole, Planar, PlanarCurve, Sketch,
    SketchConstraint, SketchEntity, SketchPlane, SketchRelation, SketchSolve,
};
use crate::design::options::{NativeOption, NativeOptions};
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
    Sketch as ModelSketch, SketchConstraint as ModelConstraint, SketchConstraintDefinition,
    SketchConstraintDefinitionInput, SketchConstraintId, SketchEntity as ModelEntity,
    SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId, SketchPlacement,
    SketchProfiles,
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
        .chain(
            ir.model
                .sketch_constraints
                .iter()
                .map(|constraint| constraint.id.as_str()),
        )
        .chain(ir.design.features.iter().map(|feature| feature.id.as_str()))
        .chain(
            ir.design
                .containers
                .iter()
                .map(|container| container.id().as_str()),
        )
        .chain(ir.design.spreadsheets.iter().map(|sheet| sheet.id.as_str()))
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

/// The model entity `key` of `ir`, and the design entity of its identity.
fn entity_pair<'ir>(
    ir: &'ir mut CadIr,
    key: &str,
) -> (&'ir mut ModelEntity, &'ir mut SketchEntity<Planar>) {
    let model = ir
        .model
        .sketch_entities
        .iter_mut()
        .find(|entity| entity.id().as_str() == key)
        .unwrap();
    let design = ir
        .design
        .sketch_entities
        .iter_mut()
        .find(|entity| entity.id.as_str() == key)
        .unwrap();
    (model, design)
}

/// A design entity that states no construction flag states `false`: it
/// carries a record flag of `false` and names a record flag of `true`. A
/// stated flag other than the record's names it.
#[test]
fn an_unstated_construction_flag_is_false() {
    let cut = |model: bool, design: Option<bool>| {
        let mut ir = sketch_fixture();
        let (model_point, design_point) = entity_pair(&mut ir, START);
        model_point.construction = model;
        design_point.construction = design;
        cuts(&ir)[START].clone()
    };
    assert_eq!(cut(false, None), fields([]));
    assert_eq!(cut(false, Some(true)), fields(["construction"]));
    assert_eq!(cut(true, None), fields(["construction"]));
}

/// A design entity carries a record curve of a native kind where its native
/// form names that kind, alone or in the namespace of a codec, and names it
/// where the form names another kind.
#[test]
fn a_native_form_carries_the_native_kind_in_a_codec_namespace() {
    let cut = |stated: &str| {
        let mut ir = sketch_fixture();
        let (model_point, design_point) = entity_pair(&mut ir, START);
        model_point.geometry = geometry(SketchGeometryDefinition::Native {
            native_kind: NonBlankString::new("conic".to_owned()).unwrap(),
        });
        design_point.definition = EntityDefinition::Defined {
            form: Box::new(EntityForm::Native(
                NonBlankString::new(stated.to_owned()).unwrap(),
            )),
            solved: None,
        };
        cuts(&ir)[START].clone()
    };
    assert_eq!(cut("conic"), fields([]));
    assert_eq!(cut("test:conic"), fields([]));
    assert_eq!(cut("test:spline"), fields(["geometry"]));
}

const CONSTRAINT: &str = "test:design:constraint#length";

/// A reference length on the fixture's line, active and of orientation 3,
/// and the design constraint that binds the line's length to a parameter of
/// `definition`, with the activity flag `active` and an orientation option
/// of `orientation`.
fn constraint_fixture(definition: ParameterDefinition, active: bool, orientation: i64) -> CadIr {
    let mut ir = sketch_fixture();
    ir.model.sketch_constraints.push(ModelConstraint {
        id: SketchConstraintId::mint(CONSTRAINT).unwrap(),
        sketch: sketch_id(),
        definition: SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::Coincident {
                entities: vec![entity_id(START), entity_id(END)],
            },
        )
        .unwrap(),
        name: None,
        driving: Some(false),
        active: Some(true),
        virtual_space: None,
        visible: None,
        orientation: Some(3),
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    });
    let mut parameter = design_parameter("length", "length");
    parameter.definition = definition;
    ir.design.parameters.push(parameter);
    ir.design
        .sketch_constraints
        .push(SketchConstraint::<Planar> {
            id: SketchConstraintId::mint(CONSTRAINT).unwrap(),
            sketch: sketch_id(),
            relation: SketchRelation::Length {
                of: vec![entity_id(LINE)],
                value: Dimension::Parameter(parameter_id("length")),
            },
            name: None,
            active: Some(Dimension::Literal(active)),
            native_options: NativeOptions::from([(
                crate::codec_name!("test:orientation"),
                NativeOption::Integer(Dimension::Literal(orientation)),
            )]),
            source_settings: NativeOptions::new(),
            native_ref: None,
        });
    ir
}

/// A design constraint states a reference by the `Driven` parameter it
/// binds, its activity as a literal, and its orientation as a
/// `<codec>:orientation` option; it names each that differs. The record's
/// definition stays named, since no typed comparison maps it to the
/// relation.
#[test]
fn a_projected_constraint_carries_its_driving_active_and_orientation_flags() {
    assert_eq!(
        cuts(&constraint_fixture(ParameterDefinition::Driven, true, 3))[CONSTRAINT],
        fields(["definition"])
    );
    assert_eq!(
        cuts(&constraint_fixture(ParameterDefinition::Unset, false, 4))[CONSTRAINT],
        fields(["active", "definition", "driving", "orientation"])
    );
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

/// Only a spreadsheet orders design parameters: two parameters of one owner
/// that no sheet lists name their ordinals, whichever order their ids take,
/// and a parameter with no other of its owner keeps its ordinal.
#[test]
fn a_parameter_ordinal_is_carried_only_where_the_design_orders_it() {
    let designs = || {
        vec![
            design_parameter("sketch:7", "sketch:7"),
            design_parameter("sketch:10", "sketch:10"),
        ]
    };
    let pair = cuts(&parameter_ir(
        vec![
            model_parameter("sketch:7", 0),
            model_parameter("sketch:10", 1),
        ],
        designs(),
    ));
    assert_eq!(pair[parameter_id("sketch:7").as_str()], fields(["ordinal"]));
    assert_eq!(
        pair[parameter_id("sketch:10").as_str()],
        fields(["ordinal"])
    );
    let lone = cuts(&parameter_ir(
        vec![model_parameter("sketch:7", 3)],
        designs(),
    ));
    assert_eq!(lone[parameter_id("sketch:7").as_str()], fields([]));
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
    let written = HashSet::from([id.as_str()]);
    assert!(same_id_cuts(&ir, &written)[id.as_str()].contains("product_definition"));
    ir.design.features[0] = feature(None);
    assert_eq!(cuts(&ir)[&id], fields(["product_definition"]));
}

/// A spreadsheet cell's position is its position among the cells of its
/// design sheet: cells the sheet lists in the order of their ordinals keep
/// them, and cells no sheet lists name them.
#[test]
fn a_cell_parameter_ordinal_follows_the_cells_of_its_sheet() {
    use crate::spreadsheets::{CellAddress, SpreadsheetCell, SpreadsheetId};
    let models = || {
        vec![
            model_parameter("sheet:cell:B1", 0),
            model_parameter("sheet:cell:A2", 1),
        ]
    };
    let designs = || {
        vec![
            design_parameter("sheet:cell:A2", "sheet:cell:A2"),
            design_parameter("sheet:cell:B1", "sheet:cell:B1"),
        ]
    };
    let unlisted = cuts(&parameter_ir(models(), designs()));
    assert_eq!(
        unlisted[parameter_id("sheet:cell:B1").as_str()],
        fields(["ordinal"])
    );
    let mut listed = parameter_ir(models(), designs());
    let cell = |row, col, key| SpreadsheetCell {
        address: CellAddress::new(row, col).unwrap(),
        parameter: parameter_id(key),
    };
    listed.design.spreadsheets.push(
        crate::design::spreadsheet::Spreadsheet::new(
            SpreadsheetId::mint("test:design:spreadsheet#sheet").unwrap(),
            vec![cell(1, 2, "sheet:cell:B1"), cell(2, 1, "sheet:cell:A2")],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
        )
        .unwrap(),
    );
    let written = listed
        .design
        .parameters
        .iter()
        .map(|parameter| parameter.id.as_str())
        .collect();
    assert!(
        same_id_cuts(&listed, &written)[parameter_id("sheet:cell:B1").as_str()].contains("ordinal")
    );
    let listed = cuts(&listed);
    assert_eq!(listed[parameter_id("sheet:cell:B1").as_str()], fields([]));
    assert_eq!(listed[parameter_id("sheet:cell:A2").as_str()], fields([]));
}

#[test]
fn a_container_carries_the_model_owner_when_written() {
    use crate::design::container::{Container, UnorderedKind};
    use crate::design::ids::ContainerId;

    let owner = FeatureId::mint("test:model:feature#node").unwrap();
    let container = ContainerId::mint("test:design:container#group").unwrap();
    let mut model = model_parameter("width", 0);
    model.owner = Some(owner.clone());
    let mut design = design_parameter("width", "width");
    design.owner = Some(ParameterOwner::Container(container.clone()));
    let mut ir = parameter_ir(vec![model], vec![design]);
    ir.model.restated.record(
        owner.as_str(),
        super::Restater::Record(container.as_str().to_owned()),
        [],
    );
    ir.design.containers.push(Container::Unordered {
        id: container,
        kind: UnorderedKind::Group,
        members: BTreeSet::new(),
    });
    let id = parameter_id("width");
    assert_eq!(cuts(&ir)[id.as_str()], fields([]));
    assert!(same_id_cuts(&ir, &HashSet::from([id.as_str()]))[id.as_str()].contains("owner"));
}

/// A model feature `key` of the operation `operation`.
fn model_feature(
    key: &str,
    operation: crate::features::FeatureOperation,
) -> crate::features::Feature {
    crate::features::Feature {
        product_definition: None,
        id: FeatureId::mint(format!("test:design:feature#{key}")).unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: crate::features::selections::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),
        evaluation: crate::features::evaluation::FeatureEvaluation::from_definition(
            crate::features::FeatureDefinition::Operation(operation),
        ),
        native_ref: None,
    }
}

/// A design parameter with no owner is a user parameter of its scope
/// (§5.12): it carries a model owner that is an equations node no design
/// record states, and names an owner that is another feature.
#[test]
fn a_user_parameter_carries_its_equations_table_owner() {
    use crate::features::tree::{FeatureTreeNodeRole, TreeChildren};
    let cut = |operation| {
        let table = model_feature("table", operation);
        let mut model = model_parameter("width", 0);
        model.owner = Some(table.id.clone());
        let mut ir = parameter_ir(vec![model], vec![design_parameter("width", "width")]);
        ir.model.features.push(table);
        cuts(&ir)[parameter_id("width").as_str()].clone()
    };
    assert_eq!(
        cut(crate::features::FeatureOperation::TreeNode {
            role: FeatureTreeNodeRole::Equations,
            children: TreeChildren::default(),
        }),
        fields([])
    );
    assert_eq!(
        cut(crate::features::FeatureOperation::TreeNode {
            role: FeatureTreeNodeRole::FeatureFolder,
            children: TreeChildren::default(),
        }),
        fields(["owner"])
    );
}

/// A design parameter with no cache carries the model value its neutral
/// definition evaluates to, through the parameters it references that have
/// none either, and names another value.
#[test]
fn an_uncached_parameter_carries_the_value_its_definition_evaluates_to() {
    use crate::design::dimension::{ParameterRef, RefTarget};
    use crate::design::expr::{parse, Expression, Neutral};
    let neutral = |text: &str, refs: &[&str]| {
        let refs = refs
            .iter()
            .map(|key| ParameterRef {
                target: RefTarget::Parameter(parameter_id(key)),
                through: None,
            })
            .collect::<Vec<_>>();
        ParameterDefinition::Expression(Expression::Neutral(Neutral(
            parse(text, &refs, &[]).unwrap(),
        )))
    };
    let charged_value = |depth: &str, write_width: bool| {
        let mut width = design_parameter("width", "width");
        width.definition = neutral("1[cm]", &[]);
        width.value = None;
        let mut design = design_parameter("depth", "depth");
        design.definition = neutral(depth, &["width"]);
        design.value = None;
        let ir = parameter_ir(vec![model_parameter("depth", 0)], vec![design, width]);
        let depth = parameter_id("depth");
        let width = parameter_id("width");
        let mut written = HashSet::from([depth.as_str()]);
        if write_width {
            written.insert(width.as_str());
        }
        same_id_cuts(&ir, &written)[depth.as_str()].contains("value")
    };
    assert!(!charged_value("$0 + 2[mm]", true));
    assert!(charged_value("$0 + 3[mm]", true));
    assert!(charged_value("$0 + 2[mm]", false));
}

/// A parameter that a radial dimension binds states no display where the
/// dimension measures the model display (D7): it carries that display, and
/// names one the dimension does not measure.
#[test]
fn a_parameter_carries_the_display_its_radial_dimension_measures() {
    use crate::design::ops::sketch::RadialMeasure;
    use crate::features::parameters::DimensionDisplay as ModelDisplay;
    let charged_display = |measure, write_constraint| {
        let mut model = model_parameter("diameter", 0);
        model.display = Some(ModelDisplay::Diameter);
        let mut ir = parameter_ir(vec![model], vec![design_parameter("diameter", "diameter")]);
        ir.design
            .sketch_constraints
            .push(SketchConstraint::<Planar> {
                id: SketchConstraintId::mint(CONSTRAINT).unwrap(),
                sketch: sketch_id(),
                relation: SketchRelation::Radial {
                    of: entity_id(LINE),
                    measure,
                    value: Dimension::Parameter(parameter_id("diameter")),
                },
                name: None,
                active: None,
                native_options: NativeOptions::new(),
                source_settings: NativeOptions::new(),
                native_ref: None,
            });
        let parameter = parameter_id("diameter");
        let mut written = HashSet::from([parameter.as_str()]);
        if write_constraint {
            written.insert(CONSTRAINT);
        }
        same_id_cuts(&ir, &written)[parameter.as_str()].contains("display")
    };
    assert!(!charged_display(RadialMeasure::Diameter, true));
    assert!(charged_display(RadialMeasure::Radius, true));
    assert!(charged_display(RadialMeasure::Diameter, false));
}
