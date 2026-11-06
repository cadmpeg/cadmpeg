// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::features::{
    edge_treatments::RadiusSpec, BooleanOp, EdgeSelection, ExtrudeExtent, FeatureDefinition,
    FeatureOperation, LinearTermination, ParameterValue, PlanarProfileRef, ProfileRef,
};
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::products::{OccurrenceParent, ProductDefinitionKind};
use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchGeometryDefinition};
use cadmpeg_ir::{Codec, DecodeOptions};

use crate::FcstdCodec;

const LENGTH_TOLERANCE: f64 = 1.0e-12;
const VOLUME_TOLERANCE: f64 = 1.0e-6;
const AREA_TOLERANCE: f64 = 1.0e-6;
const BOUNDS_TOLERANCE: f64 = 1.0e-6;

#[test]
fn authored_design_history_keeps_sketches_constraints_and_operations() {
    let bytes = include_bytes!("../../../../corpus/freecad_fcstd/fixtures/design_history.FCStd");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let model = &decoded.ir().model;
    assert_eq!(model.sketches.len(), 2);
    let sketch = |name: &str| {
        model
            .sketches
            .iter()
            .find(|sketch| sketch.name.as_deref() == Some(name))
            .unwrap()
    };
    let pad = sketch("PadSketch");
    let pocket = sketch("PocketSketch");
    let entities = |sketch: &cadmpeg_ir::sketches::Sketch| {
        model
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch.id)
            .collect::<Vec<_>>()
    };
    let rectangle = entities(pad);
    assert_eq!(rectangle.len(), 4);
    for (entity, (start, end)) in rectangle.iter().zip([
        ([0.0, 0.0], [24.0, 0.0]),
        ([24.0, 0.0], [24.0, 16.0]),
        ([24.0, 16.0], [0.0, 16.0]),
        ([0.0, 16.0], [0.0, 0.0]),
    ]) {
        let SketchGeometryDefinition::Line {
            start: actual_start,
            end: actual_end,
        } = entity.geometry.definition()
        else {
            panic!("rectangle side");
        };
        for (actual, expected) in [
            actual_start.get().u,
            actual_start.get().v,
            actual_end.get().u,
            actual_end.get().v,
        ]
        .into_iter()
        .zip(start.into_iter().chain(end))
        {
            assert!((actual - expected).abs() < LENGTH_TOLERANCE);
        }
    }
    let circle = entities(pocket);
    assert_eq!(circle.len(), 1);
    assert!(
        matches!(circle[0].geometry.definition(), SketchGeometryDefinition::Circle { center, radius }
        if (center.get().u - 12.0).abs() < LENGTH_TOLERANCE
        && (center.get().v - 8.0).abs() < LENGTH_TOLERANCE
        && (radius.get() - 3.0).abs() < LENGTH_TOLERANCE)
    );
    let (origin, _, _) = pocket.placement.resolved().unwrap();
    assert!((origin.get().z - 10.0).abs() < LENGTH_TOLERANCE);
    let pad_constraints: Vec<_> = model
        .sketch_constraints
        .iter()
        .filter(|constraint| constraint.sketch == pad.id)
        .collect();
    let pocket_constraints: Vec<_> = model
        .sketch_constraints
        .iter()
        .filter(|constraint| constraint.sketch == pocket.id)
        .collect();
    assert_eq!(pad_constraints.len(), 4);
    assert_eq!(pocket_constraints.len(), 1);
    assert!(pad_constraints.iter().any(|constraint| matches!(constraint.definition.kind(), SketchConstraintDefinitionInput::Horizontal { entity } if entity == rectangle[0].id())));
    assert!(pad_constraints.iter().any(|constraint| matches!(constraint.definition.kind(), SketchConstraintDefinitionInput::Vertical { entity } if entity == rectangle[1].id())));
    let value = |parameter: &cadmpeg_ir::features::ParameterId| {
        let parameter = model
            .parameters
            .iter()
            .find(|candidate| candidate.id == *parameter)
            .unwrap();
        let Some(ParameterValue::Length(value)) = &parameter.value else {
            panic!("length parameter")
        };
        value.get()
    };
    for (entity, expected) in [(rectangle[0], 24.0), (rectangle[1], 16.0)] {
        assert!(pad_constraints.iter().any(|constraint| matches!(constraint.definition.kind(), SketchConstraintDefinitionInput::Distance { entities, parameter } if entities.as_slice() == [entity.id().clone()] && (value(parameter) - expected).abs() < LENGTH_TOLERANCE)));
    }
    assert!(
        matches!(pocket_constraints[0].definition.kind(), SketchConstraintDefinitionInput::Diameter { entity, parameter } if entity == circle[0].id() && (value(parameter) - 6.0).abs() < LENGTH_TOLERANCE)
    );

    let operation = |name: &str| {
        let definition = model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap()
            .evaluation
            .definition();
        match definition {
            FeatureDefinition::Operation(operation)
            | FeatureDefinition::PostProcess { operation, .. } => operation,
        }
    };
    for (name, expected_sketch, expected_op, expected_length) in [
        ("Pad", &pad.id, BooleanOp::Join, 10.0),
        ("Pocket", &pocket.id, BooleanOp::Cut, 6.0),
    ] {
        assert!(
            matches!(operation(name), FeatureOperation::Extrude { profile: ProfileRef::Planar(PlanarProfileRef::Sketch(sketch)), op, extent: ExtrudeExtent::OneSided { side }, .. } if sketch == expected_sketch && *op == expected_op && matches!(side.termination, LinearTermination::Blind { length } if (length.get() - expected_length).abs() < LENGTH_TOLERANCE)),
            "{name}"
        );
    }
    let measures =
        cadmpeg_measure::test_support::round_trip::measured("design_history", decoded.ir())
            .unwrap();
    for (name, expected) in [
        ("Pad", 24.0 * 16.0 * 10.0),
        (
            "Pocket",
            24.0 * 16.0 * 10.0 - std::f64::consts::PI * 3.0 * 3.0 * 6.0,
        ),
        ("Fillet", 3830.0 - 103.0 * std::f64::consts::PI / 2.0),
    ] {
        let feature = model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap();
        let [body] = feature.evaluation.outputs().as_slice() else {
            panic!(
                "{name}: expected one result body, got {:?}",
                feature.evaluation.outputs()
            )
        };
        let body = measures
            .bodies()
            .iter()
            .find(|row| row.body() == body.as_str())
            .unwrap();
        let volume = body
            .volume()
            .as_ref()
            .unwrap()
            .measured()
            .unwrap()
            .value()
            .value
            .get();
        assert!(
            (volume - expected).abs() < VOLUME_TOLERANCE,
            "{name}: volume {volume}"
        );
    }
    assert!(
        matches!(operation("Fillet"), FeatureOperation::Fillet { groups } if matches!(groups.as_slice(), [group] if matches!(group.radius, RadiusSpec::Constant { radius } if (radius.get() - 1.0).abs() < LENGTH_TOLERANCE)))
    );
    let properties = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let base = properties
        .iter()
        .find(|property| {
            property.owner == crate::native::native_id("object", "Fillet")
                && property.name == "Base"
        })
        .unwrap();
    let [Some(link)] = base.links() else {
        panic!("Fillet.Base link")
    };
    assert_eq!(link.document(), None);
    let pocket_id = crate::native::native_id("object", "Pocket");
    assert_eq!(link.object(), Some(pocket_id.as_str()));
    assert_eq!(link.subelements(), ["Edge1"]);
    let FeatureOperation::Fillet { groups } = operation("Fillet") else {
        panic!("Fillet operation")
    };
    let [group] = groups.as_slice() else {
        panic!("Fillet group")
    };
    let EdgeSelection::Resolved { edges, native } = &group.edges else {
        panic!("Fillet.Edge1 selection is unresolved")
    };
    assert_eq!(native, &base.id);
    let [selected] = edges.as_slice() else {
        panic!("Fillet.Edge1 selection count")
    };
    let edge = model
        .edges
        .iter()
        .find(|edge| &edge.id == selected)
        .unwrap();
    let vertex_position = |id: &cadmpeg_ir::ids::VertexId| {
        let vertex = model
            .vertices
            .iter()
            .find(|vertex| &vertex.id == id)
            .unwrap();
        model
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .unwrap()
            .position()
            .get()
    };
    let start = vertex_position(&edge.start);
    let end = vertex_position(&edge.end);
    let at_bottom = |point: cadmpeg_ir::math::Point3| {
        point.x.abs() < LENGTH_TOLERANCE
            && point.y.abs() < LENGTH_TOLERANCE
            && point.z.abs() < LENGTH_TOLERANCE
    };
    let at_top = |point: cadmpeg_ir::math::Point3| {
        point.x.abs() < LENGTH_TOLERANCE
            && point.y.abs() < LENGTH_TOLERANCE
            && (point.z - 10.0).abs() < LENGTH_TOLERANCE
    };
    assert!(
        (at_bottom(start) && at_top(end)) || (at_top(start) && at_bottom(end)),
        "Fillet.Edge1 endpoints: {start:?}, {end:?}"
    );
    let curve_id = edge.curve().unwrap();
    let curve = model
        .curves
        .iter()
        .find(|curve| &curve.id == curve_id)
        .unwrap();
    let Some(SolvedCurveGeometry::Line(line)) = curve.geometry.solved() else {
        panic!("Fillet.Edge1 is not a line")
    };
    let direction = *line.direction().as_raw();
    assert!(
        direction.x.abs() < LENGTH_TOLERANCE
            && direction.y.abs() < LENGTH_TOLERANCE
            && (direction.z.abs() - 1.0).abs() < LENGTH_TOLERANCE,
        "Fillet.Edge1 direction: {direction:?}"
    );
}

#[test]
fn authored_core_product_keeps_primitive_dimensions_and_results() {
    use cadmpeg_ir::features::{BooleanKind, PrimitiveSolidKind};
    use cadmpeg_measure::report::RowPlacement;

    let bytes =
        include_bytes!("../../../../corpus/freecad_fcstd/fixtures/core_design_product.FCStd");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let model = &decoded.ir().model;
    let feature = |name: &str| {
        model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap()
    };
    let operation = |name: &str| match feature(name).evaluation.definition() {
        FeatureDefinition::Operation(operation)
        | FeatureDefinition::PostProcess { operation, .. } => operation,
    };
    let FeatureOperation::Primitive { solid, .. } = operation("Box") else {
        panic!("box primitive")
    };
    assert!(
        matches!(solid.kind(), PrimitiveSolidKind::Box { length, width, height } if (length.get() - 30.0).abs() < LENGTH_TOLERANCE && (width.get() - 20.0).abs() < LENGTH_TOLERANCE && (height.get() - 10.0).abs() < LENGTH_TOLERANCE)
    );
    let FeatureOperation::Primitive {
        solid,
        placement: Some(placement),
        ..
    } = operation("Cylinder")
    else {
        panic!("cylinder primitive and frame")
    };
    assert!(
        matches!(solid.kind(), PrimitiveSolidKind::Cylinder { radius, height, angle } if (radius.get() - 4.0).abs() < LENGTH_TOLERANCE && (height.get() - 16.0).abs() < LENGTH_TOLERANCE && (angle.get() - std::f64::consts::TAU).abs() < LENGTH_TOLERANCE)
    );
    for (actual, expected) in [
        placement.rows()[0][3],
        placement.rows()[1][3],
        placement.rows()[2][3],
    ]
    .into_iter()
    .zip([15.0, 10.0, 0.0])
    {
        assert!((actual - expected).abs() < LENGTH_TOLERANCE);
    }
    assert!(matches!(
        operation("Cut"),
        FeatureOperation::Combine {
            op: BooleanKind::Cut,
            ..
        }
    ));
    assert_eq!(
        feature("Cut").dependencies.iter().collect::<Vec<_>>(),
        [&feature("Box").id, &feature("Cylinder").id]
    );
    assert!(matches!(
        operation("DesignedFeature"),
        FeatureOperation::StoredGeometry {}
    ));
    let properties = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let length = properties
        .iter()
        .find(|property| {
            property.owner == crate::native::native_id("object", "DesignedFeature")
                && property.name == "Length"
        })
        .unwrap();
    let [value] = length.values() else {
        panic!("one authored feature length")
    };
    let length = value.attributes["value"].parse::<f64>().unwrap();
    assert!((length - 5.0).abs() < LENGTH_TOLERANCE);
    let [profile] = model.sketches.as_slice() else {
        panic!("expected one profile sketch");
    };
    assert_eq!(profile.name.as_deref(), Some("Profile"));
    let geometry: Vec<_> = model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == profile.id)
        .collect();
    assert_eq!(geometry.len(), 4);
    for (entity, (start, end)) in geometry.iter().zip([
        ([0.0, 0.0], [12.0, 0.0]),
        ([12.0, 0.0], [12.0, 8.0]),
        ([12.0, 8.0], [0.0, 8.0]),
        ([0.0, 8.0], [0.0, 0.0]),
    ]) {
        let SketchGeometryDefinition::Line {
            start: actual_start,
            end: actual_end,
        } = entity.geometry.definition()
        else {
            panic!("profile side");
        };
        for (actual, expected) in [
            actual_start.get().u,
            actual_start.get().v,
            actual_end.get().u,
            actual_end.get().v,
        ]
        .into_iter()
        .zip(start.into_iter().chain(end))
        {
            assert!((actual - expected).abs() < LENGTH_TOLERANCE);
        }
    }
    let constraints: Vec<_> = model
        .sketch_constraints
        .iter()
        .filter(|constraint| constraint.sketch == profile.id)
        .collect();
    assert_eq!(constraints.len(), 4);
    assert!(constraints.iter().any(|constraint| matches!(constraint.definition.kind(), SketchConstraintDefinitionInput::Horizontal { entity } if entity == geometry[0].id())));
    assert!(constraints.iter().any(|constraint| matches!(constraint.definition.kind(), SketchConstraintDefinitionInput::Vertical { entity } if entity == geometry[1].id())));
    for (entity, expected) in [(geometry[0], 12.0), (geometry[1], 8.0)] {
        assert!(constraints.iter().any(|constraint| matches!(constraint.definition.kind(), SketchConstraintDefinitionInput::Distance { entities, parameter } if entities.as_slice() == [entity.id().clone()] && model.parameters.iter().any(|candidate| candidate.id == *parameter && matches!(&candidate.value, Some(ParameterValue::Length(value)) if (value.get() - expected).abs() < LENGTH_TOLERANCE)))));
    }
    assert!(feature("DesignedFeature")
        .dependencies
        .iter()
        .any(|id| id == &feature("Profile").id));
    let product = model
        .product_definitions
        .iter()
        .find(|definition| definition.source_name.as_deref() == Some("Product"))
        .unwrap();
    assert!(matches!(&product.kind, ProductDefinitionKind::Part));
    assert_eq!(product.label.as_deref(), Some("CC0 Product"));
    let product_occurrence = model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.id.as_str() == "fcstd:model:occurrence#Product:container")
        .unwrap();
    assert!(matches!(
        &product_occurrence.parent,
        OccurrenceParent::Root {}
    ));
    assert!(matches!(
        &product_occurrence.prototype,
        cadmpeg_ir::products::PrototypeReference::Local { definition }
            if definition == &product.id
    ));
    for (ordinal, name) in ["Box", "Cylinder", "Cut", "Profile", "DesignedFeature"]
        .into_iter()
        .enumerate()
    {
        let child = model
            .occurrences
            .iter()
            .find(|occurrence| {
                occurrence.native_ref.as_deref()
                    == Some(crate::native::native_id("object", name).as_str())
            })
            .unwrap();
        assert!(matches!(
            &child.parent,
            OccurrenceParent::Occurrence { occurrence } if occurrence == &product_occurrence.id
        ));
        assert_eq!(child.ordinal, u32::try_from(ordinal).unwrap());
    }
    let cylinder_occurrence = model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.id.as_str().ends_with("#Cylinder:container"))
        .unwrap();
    let frame = cylinder_occurrence.effective_transform().unwrap().rows();
    for (actual, expected) in [frame[0][3], frame[1][3], frame[2][3]]
        .into_iter()
        .zip([15.0, 10.0, 0.0])
    {
        assert!((actual - expected).abs() < LENGTH_TOLERANCE);
    }
    let occurrence = model
        .occurrences
        .iter()
        .find(|occurrence| {
            occurrence
                .native_ref
                .as_deref()
                .is_some_and(|id| id.ends_with("#Occurrence"))
        })
        .unwrap();
    assert!(matches!(&occurrence.parent, OccurrenceParent::Root {}));
    let cadmpeg_ir::products::PrototypeReference::Local { definition } = &occurrence.prototype
    else {
        panic!("local occurrence prototype");
    };
    let prototype = model
        .product_definitions
        .iter()
        .find(|prototype| &prototype.id == definition)
        .unwrap();
    assert_eq!(prototype.source_name.as_deref(), Some("DesignedFeature"));
    for (actual, expected) in [
        occurrence.transform.rows()[0][3],
        occurrence.transform.rows()[1][3],
        occurrence.transform.rows()[2][3],
    ]
    .into_iter()
    .zip([40.0, 0.0, 0.0])
    {
        assert!((actual - expected).abs() < LENGTH_TOLERANCE);
    }
    assert_eq!(model.spreadsheets.len(), 1);
    let sheet = &model.spreadsheets[0];
    assert_eq!(sheet.cells().len(), 4);
    for (address, expected) in [("A1", "Length"), ("A2", "Width")] {
        let cell = sheet
            .cells()
            .iter()
            .find(|cell| cell.address.a1() == address)
            .unwrap();
        let parameter = model
            .parameters
            .iter()
            .find(|parameter| parameter.id == cell.parameter)
            .unwrap();
        assert!(
            matches!(&parameter.value, Some(ParameterValue::String(value)) if value == expected)
        );
    }
    for (address, expected) in [("B1", 30.0), ("B2", 20.0)] {
        let cell = sheet
            .cells()
            .iter()
            .find(|cell| cell.address.a1() == address)
            .unwrap();
        let parameter = model
            .parameters
            .iter()
            .find(|parameter| parameter.id == cell.parameter)
            .unwrap();
        let Some(ParameterValue::Length(value)) = &parameter.value else {
            panic!("{address}: length")
        };
        if address == "B1" {
            assert_eq!(parameter.name, "ProductLength");
        }
        assert!((value.get() - expected).abs() < LENGTH_TOLERANCE);
    }
    let report =
        cadmpeg_measure::test_support::round_trip::measured("core_design_product", decoded.ir())
            .unwrap();
    for (name, expected_volume, expected_area, bounds) in [
        (
            "Box",
            30.0 * 20.0 * 10.0,
            2.0 * (30.0 * 20.0 + 20.0 * 10.0 + 30.0 * 10.0),
            [0.0, 0.0, 0.0, 30.0, 20.0, 10.0],
        ),
        (
            "Cylinder",
            std::f64::consts::PI * 4.0 * 4.0 * 16.0,
            2.0 * std::f64::consts::PI * 4.0 * (4.0 + 16.0),
            [-4.0, -4.0, 0.0, 4.0, 4.0, 16.0],
        ),
        (
            "Cut",
            30.0 * 20.0 * 10.0 - std::f64::consts::PI * 4.0 * 4.0 * 10.0,
            2.0 * (30.0 * 20.0 + 20.0 * 10.0 + 30.0 * 10.0)
                + 2.0 * std::f64::consts::PI * 4.0 * (10.0 - 4.0),
            [0.0, 0.0, 0.0, 30.0, 20.0, 10.0],
        ),
        (
            "DesignedFeature",
            12.0 * 8.0 * 5.0,
            2.0 * (12.0 * 8.0 + 8.0 * 5.0 + 12.0 * 5.0),
            [0.0, 0.0, 0.0, 12.0, 8.0, 5.0],
        ),
    ] {
        let [body] = feature(name).evaluation.outputs().as_slice() else {
            panic!("{name}: expected one body")
        };
        let row = report
            .bodies()
            .iter()
            .find(|row| {
                row.body() == body.as_str() && matches!(row.placement(), RowPlacement::Model {})
            })
            .unwrap();
        let actual_volume = row
            .volume()
            .as_ref()
            .unwrap()
            .measured()
            .unwrap()
            .value()
            .value
            .get();
        assert!(
            (actual_volume - expected_volume).abs() < VOLUME_TOLERANCE,
            "{name}: volume {actual_volume}"
        );
        let actual_area = row
            .area()
            .as_ref()
            .unwrap()
            .measured()
            .unwrap()
            .value()
            .value
            .get();
        assert!(
            (actual_area - expected_area).abs() < AREA_TOLERANCE,
            "{name}: area {actual_area}"
        );
        let bbox = row.bbox().measured().unwrap().value();
        for (actual, expected) in bbox
            .min
            .coordinates()
            .into_iter()
            .chain(bbox.max.coordinates())
            .zip(bounds)
        {
            assert!(
                (actual.get() - expected).abs() < BOUNDS_TOLERANCE,
                "{name}: bounding box"
            );
        }
    }
}
