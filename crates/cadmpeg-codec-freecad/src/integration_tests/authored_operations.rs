// SPDX-License-Identifier: Apache-2.0

use crate::FcstdCodec;
use cadmpeg_ir::features::{EdgeSelection, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::sketches::SketchGeometryDefinition;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

const LENGTH_TOLERANCE: f64 = 1.0e-12;
const ANGLE_TOLERANCE: f64 = 1.0e-12;
const DIRECTION_TOLERANCE: f64 = 1.0e-12;

#[test]
fn authored_core_operations_keep_feature_operands_and_parameters() {
    let bytes = include_bytes!("../../../../corpus/freecad_fcstd/fixtures/core_operations.FCStd");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let model = &decoded.ir().model;
    let feature = |name: &str| {
        model
            .features
            .iter()
            .find(|f| f.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
    };
    let operation = |name: &str| {
        let (FeatureDefinition::Operation(op)
        | FeatureDefinition::PostProcess { operation: op, .. }) =
            feature(name).evaluation.definition();
        serde_json::to_value(op).unwrap()
    };
    for (name, kind, owners) in [
        ("Revolution", "revolve", &["RevolutionProfile"][..]),
        ("SymmetricExtrusion", "extrude", &["ExtrusionProfile"][..]),
        ("Loft", "loft", &["LoftSectionA", "LoftSectionB"][..]),
        ("Sweep", "sweep", &["SweepSection", "SweepSpine"][..]),
        ("PartFillet", "fillet", &["DressupBase"][..]),
        ("PartChamfer", "chamfer", &["ChamferBase"][..]),
        ("Thickness", "shell", &["ThicknessBase"][..]),
        ("Mirror", "mirror_shape", &["MirrorBase"][..]),
        (
            "Draft",
            "draft",
            &["DraftBase", "DatumPlane", "DatumAxis"][..],
        ),
        ("Hole", "hole", &["HoleProfile", "HoleBase"][..]),
        (
            "LinearPattern",
            "pattern",
            &["PatternBase", "DatumAxis"][..],
        ),
    ] {
        let op = operation(name);
        assert_eq!(op["definition"].as_str(), Some(kind), "{name}");
        for owner in owners {
            assert!(
                feature(name)
                    .dependencies
                    .iter()
                    .any(|id| id == &feature(owner).id),
                "{name}: {owner}"
            );
        }
    }
    for (name, pointer, expected, tolerance) in [
        (
            "Revolution",
            "/construction/extent/termination/angle",
            225.0_f64.to_radians(),
            ANGLE_TOLERANCE,
        ),
        (
            "Revolution",
            "/construction/axis/direction/x",
            0.0,
            DIRECTION_TOLERANCE,
        ),
        (
            "Revolution",
            "/construction/axis/direction/y",
            1.0,
            DIRECTION_TOLERANCE,
        ),
        (
            "Revolution",
            "/construction/axis/direction/z",
            0.0,
            DIRECTION_TOLERANCE,
        ),
        (
            "SymmetricExtrusion",
            "/extent/side/termination/length",
            12.0,
            LENGTH_TOLERANCE,
        ),
        (
            "SymmetricExtrusion",
            "/extent/side/draft",
            3.0_f64.to_radians(),
            ANGLE_TOLERANCE,
        ),
        (
            "SymmetricExtrusion",
            "/direction/vector/x",
            0.0,
            DIRECTION_TOLERANCE,
        ),
        (
            "SymmetricExtrusion",
            "/direction/vector/y",
            0.0,
            DIRECTION_TOLERANCE,
        ),
        (
            "SymmetricExtrusion",
            "/direction/vector/z",
            1.0,
            DIRECTION_TOLERANCE,
        ),
        (
            "PartFillet",
            "/groups/0/radius/radius",
            2.0,
            LENGTH_TOLERANCE,
        ),
        ("PartChamfer", "/groups/0/spec/first", 1.5, LENGTH_TOLERANCE),
        (
            "PartChamfer",
            "/groups/0/spec/second",
            2.5,
            LENGTH_TOLERANCE,
        ),
        ("Thickness", "/thickness", 1.25, LENGTH_TOLERANCE),
        ("Mirror", "/plane_normal/x", 1.0, DIRECTION_TOLERANCE),
        ("Mirror", "/plane_normal/y", 0.0, DIRECTION_TOLERANCE),
        ("Mirror", "/plane_normal/z", 0.0, DIRECTION_TOLERANCE),
        ("Draft", "/angle", -5.0_f64.to_radians(), ANGLE_TOLERANCE),
        ("Hole", "/shape/diameter", 6.0, LENGTH_TOLERANCE),
        ("Hole", "/extent/length", 7.0, LENGTH_TOLERANCE),
        (
            "Hole",
            "/shape/construction/kind/diameter",
            10.0,
            LENGTH_TOLERANCE,
        ),
        (
            "Hole",
            "/shape/construction/kind/angle",
            82.0_f64.to_radians(),
            ANGLE_TOLERANCE,
        ),
        (
            "Hole",
            "/bottom/included_angle",
            118.0_f64.to_radians(),
            ANGLE_TOLERANCE,
        ),
        (
            "LinearPattern",
            "/pattern/spacing",
            24.0 / 3.0,
            LENGTH_TOLERANCE,
        ),
        (
            "LinearPattern",
            "/pattern/direction/x",
            0.0,
            DIRECTION_TOLERANCE,
        ),
        (
            "LinearPattern",
            "/pattern/direction/y",
            0.0,
            DIRECTION_TOLERANCE,
        ),
        (
            "LinearPattern",
            "/pattern/direction/z",
            -1.0,
            DIRECTION_TOLERANCE,
        ),
        ("DatumPlane", "/frame/origin/z", 4.0, LENGTH_TOLERANCE),
        ("DatumAxis", "/origin/x", 2.0, LENGTH_TOLERANCE),
        ("DatumAxis", "/origin/y", 3.0, LENGTH_TOLERANCE),
        ("DatumPoint", "/position/x", 4.0, LENGTH_TOLERANCE),
        ("DatumPoint", "/position/y", 5.0, LENGTH_TOLERANCE),
        ("DatumPoint", "/position/z", 6.0, LENGTH_TOLERANCE),
        (
            "DatumCoordinateSystem",
            "/frame/origin/x",
            7.0,
            LENGTH_TOLERANCE,
        ),
        (
            "DatumCoordinateSystem",
            "/frame/origin/y",
            8.0,
            LENGTH_TOLERANCE,
        ),
        (
            "DatumCoordinateSystem",
            "/frame/origin/z",
            9.0,
            LENGTH_TOLERANCE,
        ),
        ("PatternBase", "/solid/length", 4.0, LENGTH_TOLERANCE),
        ("PatternBase", "/solid/width", 4.0, LENGTH_TOLERANCE),
        ("PatternBase", "/solid/height", 8.0, LENGTH_TOLERANCE),
    ] {
        let op = operation(name);
        let actual = op
            .pointer(pointer)
            .and_then(serde_json::Value::as_f64)
            .unwrap_or_else(|| panic!("{name}: {pointer}"));
        assert!((actual - expected).abs() < tolerance, "{name}: {pointer}");
    }
    for (name, pointer, expected) in [
        ("Revolution", "/construction/extent/kind", "symmetric"),
        ("SymmetricExtrusion", "/extent/kind", "symmetric"),
        ("Sweep", "/orientation/kind", "frenet"),
        ("Sweep", "/transition", "transformed"),
        ("Sweep", "/shape/mode", "solid"),
        ("PartChamfer", "/groups/0/spec/kind", "two_distances"),
        ("Thickness", "/mode", "skin"),
        ("Thickness", "/join", "arc"),
        ("Hole", "/shape/construction/kind/kind", "countersink"),
        ("Hole", "/bottom/kind", "angled"),
        ("LinearPattern", "/pattern/kind", "linear"),
    ] {
        assert_eq!(
            operation(name)
                .pointer(pointer)
                .and_then(serde_json::Value::as_str),
            Some(expected),
            "{name}: {pointer}"
        );
    }
    for (name, pointer, expected) in [
        ("Revolution", "/construction/solid", false),
        ("SymmetricExtrusion", "/solid", true),
        ("Loft", "/solid", true),
        ("Loft", "/ruled", true),
        ("Loft", "/closed", false),
        ("Thickness", "/resolve_intersections", true),
    ] {
        assert_eq!(
            operation(name)
                .pointer(pointer)
                .and_then(serde_json::Value::as_bool),
            Some(expected),
            "{name}: {pointer}"
        );
    }
    assert_eq!(operation("Loft")["max_degree"].as_u64(), Some(4));
    assert_eq!(operation("Loft")["sections"].as_array().unwrap().len(), 2);
    assert_eq!(
        operation("PartFillet")["groups"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        operation("PartChamfer")["groups"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        operation("Draft")["faces"]["value"]["faces"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        operation("Thickness")["removed_faces"]["value"]["faces"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        operation("LinearPattern")["pattern"]["count"].as_u64(),
        Some(4)
    );
    for (name, base) in [
        ("PartFillet", "DressupBase"),
        ("PartChamfer", "ChamferBase"),
    ] {
        let (FeatureDefinition::Operation(op)
        | FeatureDefinition::PostProcess { operation: op, .. }) =
            feature(name).evaluation.definition();
        let selection = match op {
            FeatureOperation::Fillet { groups } => &groups.as_slice()[0].edges,
            FeatureOperation::Chamfer { groups, .. } => &groups.as_slice()[0].edges,
            _ => panic!("{name}: dress-up"),
        };
        let EdgeSelection::Resolved { edges, .. } = selection else {
            panic!("{name}: source edge is unresolved")
        };
        let [edge] = edges.as_slice() else {
            panic!("{name}: expected one selected edge")
        };
        let [body] = feature(base).evaluation.outputs().as_slice() else {
            panic!("{base}: one body")
        };
        let body = model
            .bodies
            .iter()
            .find(|candidate| &candidate.id == body)
            .unwrap();
        let in_body = body
            .regions
            .iter()
            .filter_map(|id| model.regions.iter().find(|item| &item.id == id))
            .flat_map(|region| &region.shells)
            .filter_map(|id| model.shells.iter().find(|item| &item.id == id))
            .flat_map(cadmpeg_ir::topology::Shell::faces)
            .filter_map(|id| model.faces.iter().find(|item| &item.id == id))
            .flat_map(|face| face.loops.iter())
            .filter_map(|id| model.loops.iter().find(|item| &item.id == id))
            .flat_map(cadmpeg_ir::topology::Loop::coedges)
            .filter_map(|id| model.coedges.iter().find(|item| &item.id == id))
            .any(|coedge| &coedge.edge == edge);
        assert!(in_body, "{name}: selected edge belongs to {base}");
    }
    let sketch = |name: &str| {
        model
            .sketches
            .iter()
            .find(|s| s.name.as_deref() == Some(name))
            .unwrap()
    };
    let profile = sketch("RevolutionProfile");
    let lines: Vec<_> = model
        .sketch_entities
        .iter()
        .filter(|e| e.sketch == profile.id)
        .collect();
    let [line] = lines.as_slice() else {
        panic!("revolution profile")
    };
    let SketchGeometryDefinition::Line { start, end } = line.geometry.definition() else {
        panic!("line")
    };
    for (actual, expected) in [start.get().u, start.get().v, end.get().u, end.get().v]
        .into_iter()
        .zip([4.0, 0.0, 8.0, 0.0])
    {
        assert!((actual - expected).abs() < LENGTH_TOLERANCE);
    }
    let profile = sketch("HoleProfile");
    let circles: Vec<_> = model
        .sketch_entities
        .iter()
        .filter(|e| e.sketch == profile.id)
        .collect();
    let [circle] = circles.as_slice() else {
        panic!("hole profile")
    };
    let SketchGeometryDefinition::Circle { center, radius } = circle.geometry.definition() else {
        panic!("circle")
    };
    for (actual, expected) in [center.get().u, center.get().v, radius.get()]
        .into_iter()
        .zip([12.0, 9.0, 3.0])
    {
        assert!((actual - expected).abs() < LENGTH_TOLERANCE);
    }
    let (origin, _, _) = profile.placement.resolved().unwrap();
    assert!((origin.get().z - 10.0).abs() < LENGTH_TOLERANCE);
}
#[test]
fn authored_core_operation_bodies_match_source_dimensions() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_measure::{MeasureInput, MeasureOptions, Measurer, Selection};

    const VOLUME_TOLERANCE: f64 = 1.0e-5;
    const AREA_TOLERANCE: f64 = 1.0e-5;
    const BOX_TOLERANCE: f64 = 1.0e-6;
    let bytes = include_bytes!("../../../../corpus/freecad_fcstd/fixtures/core_operations.FCStd");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let model = &decoded.ir().model;
    let feature = |name: &str| {
        model
            .features
            .iter()
            .find(|f| f.name.as_deref() == Some(name))
            .unwrap()
    };
    let pi = std::f64::consts::PI;
    let box_measures = |x: f64, y: f64, z: f64| (x * y * z, 2.0 * (x * y + x * z + y * z));
    let rows = [
        (
            "DressupBase",
            box_measures(20.0, 16.0, 10.0),
            [0.0, 0.0, 0.0, 20.0, 16.0, 10.0],
        ),
        (
            "ChamferBase",
            box_measures(18.0, 12.0, 9.0),
            [0.0, 0.0, 0.0, 18.0, 12.0, 9.0],
        ),
        (
            "ThicknessBase",
            box_measures(16.0, 14.0, 8.0),
            [0.0, 0.0, 0.0, 16.0, 14.0, 8.0],
        ),
        (
            "MirrorBase",
            (pi * 3.0 * 3.0 * 10.0, 2.0 * pi * 3.0 * (3.0 + 10.0)),
            [9.0, -3.0, 0.0, 15.0, 3.0, 10.0],
        ),
        (
            "DraftBase",
            box_measures(20.0, 15.0, 10.0),
            [0.0, 0.0, 0.0, 20.0, 15.0, 10.0],
        ),
        (
            "HoleBase",
            box_measures(24.0, 18.0, 10.0),
            [0.0, 0.0, 0.0, 24.0, 18.0, 10.0],
        ),
        (
            "PatternBase",
            box_measures(4.0, 4.0, 8.0),
            [0.0, 0.0, 0.0, 4.0, 4.0, 8.0],
        ),
        (
            "Loft",
            (pi * 4.0 * 4.0 * 14.0, 2.0 * pi * 4.0 * (4.0 + 14.0)),
            [-4.0, -4.0, 0.0, 4.0, 4.0, 14.0],
        ),
    ];
    let bodies = rows
        .iter()
        .map(|(name, _, _)| {
            let [id] = feature(name).evaluation.outputs().as_slice() else {
                panic!("{name}: expected one body")
            };
            id.clone()
        })
        .collect::<Vec<_>>();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
    let report = Measurer::new(
        MeasureInput {
            ir: decoded.ir(),
            fidelity: None,
            geometry_transferred: true,
        },
        MeasureOptions::default(),
    )
    .measure(
        &ctx,
        &Selection {
            bodies: bodies.clone(),
            ..Selection::default()
        },
    )
    .unwrap();
    for ((name, (expected_volume, expected_area), bounds), id) in rows.into_iter().zip(bodies) {
        let body = report
            .bodies()
            .iter()
            .find(|body| body.body() == id.as_str())
            .unwrap();
        let volume = body.volume().as_ref().unwrap().measured().unwrap();
        let area = body.area().as_ref().unwrap().measured().unwrap();
        assert!(
            (volume.value().value.get() - expected_volume).abs() < VOLUME_TOLERANCE,
            "{name}: volume"
        );
        assert!(
            (area.value().value.get() - expected_area).abs() < AREA_TOLERANCE,
            "{name}: area"
        );
        let bbox = body.bbox().measured().unwrap().value();
        for (actual, expected) in bbox
            .min
            .coordinates()
            .into_iter()
            .chain(bbox.max.coordinates())
            .zip(bounds)
        {
            assert!(
                (actual.get() - expected).abs() < BOX_TOLERANCE,
                "{name}: bbox"
            );
        }
    }
    ctx.finish_session().unwrap();
}
