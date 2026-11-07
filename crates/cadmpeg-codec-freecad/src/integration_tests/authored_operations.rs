// SPDX-License-Identifier: Apache-2.0

use crate::FcstdCodec;
use cadmpeg_ir::features::{
    patterns::PatternSeed, BodySelection, EdgeSelection, FaceSelection, FeatureDefinition,
    FeatureOperation, PathRef, PlanarProfileRef,
};
use cadmpeg_ir::sketches::SketchGeometryDefinition;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

const LENGTH_TOLERANCE: f64 = 1.0e-12;
const ANGLE_TOLERANCE: f64 = 1.0e-12;
const DIRECTION_TOLERANCE: f64 = 1.0e-12;

fn shape_occurrences(
    decoded: &cadmpeg_ir::DecodeResult,
) -> Vec<crate::topology_transfer::TopologyOccurrence> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let namespace = decoded.ir().native.namespace("fcstd").unwrap();
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let payloads = namespace
        .arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")
        .unwrap();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
    let mut ir = decoded.ir().clone();
    crate::topology_transfer::transfer(&ctx, &mut ir, &payloads, &properties, &mut Vec::new())
        .unwrap()
}

fn operation_definition(feature: &cadmpeg_ir::features::Feature) -> &FeatureOperation {
    match feature.evaluation.definition() {
        FeatureDefinition::Operation(operation)
        | FeatureDefinition::PostProcess { operation, .. } => operation,
    }
}

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
    let operation = |name: &str| serde_json::to_value(operation_definition(feature(name))).unwrap();
    let native_properties = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let native_property = |owner: &str, name: &str| {
        native_properties
            .iter()
            .find(|property| {
                property.owner == crate::native::native_id("object", owner) && property.name == name
            })
            .unwrap_or_else(|| panic!("{owner}.{name}: native property"))
    };
    let assert_link = |property: &crate::native::PropertyRecord,
                       expected_object: &str,
                       expected_subelements: &[&str]| {
        let [Some(link)] = property.links() else {
            panic!("{} link", property.name)
        };
        assert_eq!(link.document(), None, "{} document", property.name);
        assert_eq!(
            link.object(),
            Some(expected_object),
            "{} target",
            property.name
        );
        assert_eq!(
            link.subelements(),
            expected_subelements,
            "{} selected subelements",
            property.name
        );
    };
    let native_entries = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let assert_part_edge = |property: &crate::native::PropertyRecord, expected: [f64; 2]| {
        let [entry_name] = property.side_entries() else {
            panic!("{}.Edges side entry", property.owner)
        };
        let entry = native_entries
            .iter()
            .find(|entry| entry.name() == entry_name)
            .unwrap_or_else(|| panic!("{}.Edges payload", property.owner));
        let data = entry.data();
        assert_eq!(u32::from_le_bytes(data[0..4].try_into().unwrap()), 1);
        assert_eq!(i32::from_le_bytes(data[4..8].try_into().unwrap()), 1);
        for (bytes, expected) in data[8..24].chunks_exact(8).zip(expected) {
            assert_eq!(
                f64::from_le_bytes(bytes.try_into().unwrap()),
                expected,
                "{}.Edges authored treatment",
                property.owner
            );
        }
        assert_eq!(data.len(), 24, "{}.Edges payload size", property.owner);
    };
    let occurrences = shape_occurrences(&decoded);
    let expected_topology_id = |shape: &crate::native::PropertyRecord, name: &str, index| {
        occurrences
            .iter()
            .find(|occurrence| {
                occurrence.property == shape.id
                    && occurrence.indexed_name == name
                    && occurrence.source_index == index
            })
            .unwrap_or_else(|| panic!("{}.{}{} topology occurrence", shape.owner, name, index))
            .topology_id
            .as_str()
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
    let revolution_source = native_property("Revolution", "Source");
    let revolution_profile_native_id = crate::native::native_id("object", "RevolutionProfile");
    assert_link(
        revolution_source,
        revolution_profile_native_id.as_str(),
        &[],
    );
    let revolution_sketch = model
        .sketches
        .iter()
        .find(|sketch| sketch.name.as_deref() == Some("RevolutionProfile"))
        .unwrap();
    let revolution_feature = feature("Revolution");
    let FeatureOperation::Revolve {
        construction: cadmpeg_ir::features::RevolveConstruction::Resolved { profile, .. },
        ..
    } = operation_definition(revolution_feature)
    else {
        panic!(
            "Revolution profile resolves; decoded definition: {:#?}",
            revolution_feature.evaluation.definition()
        )
    };
    assert_eq!(
        profile,
        &PlanarProfileRef::Sketch(revolution_sketch.id.clone())
    );

    let extrusion_base = native_property("SymmetricExtrusion", "Base");
    let extrusion_profile_id = feature("ExtrusionProfile").id.as_str();
    assert_eq!(
        operation("SymmetricExtrusion")["profile"]["kind"].as_str(),
        Some("feature"),
        "extrusion profile resolves to a typed feature result"
    );
    assert_eq!(
        operation("SymmetricExtrusion")["profile"]["value"].as_str(),
        Some(extrusion_profile_id),
        "extrusion profile feature"
    );
    let [Some(extrusion_profile)] = extrusion_base.links() else {
        panic!("SymmetricExtrusion.Base link")
    };
    let extrusion_profile_native_id = crate::native::native_id("object", "ExtrusionProfile");
    assert_eq!(
        extrusion_profile.object(),
        Some(extrusion_profile_native_id.as_str()),
        "extrusion Base points to its authored profile"
    );

    let loft_sections = native_property("Loft", "Sections");
    let loft_section_links = loft_sections.links();
    assert_eq!(loft_section_links.len(), 2, "Loft.Sections count");
    let expected_loft_sections = ["LoftSectionA", "LoftSectionB"];
    let loft_operation = operation("Loft");
    let actual_loft_sections = loft_operation["sections"].as_array().unwrap();
    assert_eq!(actual_loft_sections.len(), expected_loft_sections.len());
    for ((section, link), expected) in actual_loft_sections
        .iter()
        .zip(loft_section_links)
        .zip(expected_loft_sections)
    {
        assert_eq!(section["kind"].as_str(), Some("feature"));
        let expected_feature_id = feature(expected).id.as_str();
        assert_eq!(
            section["value"].as_str(),
            Some(expected_feature_id),
            "Loft section typed feature"
        );
        let expected_section_id = crate::native::native_id("object", expected);
        assert_eq!(
            link.as_ref().and_then(|link| link.object()),
            Some(expected_section_id.as_str()),
            "Loft section order"
        );
    }

    let sweep = operation("Sweep");
    let sweep_section_feature_id = feature("SweepSection").id.as_str();
    assert_eq!(
        sweep["shape"]["section"]["value"]["kind"].as_str(),
        Some("feature"),
        "Sweep profile resolves to a typed feature result"
    );
    assert_eq!(
        sweep["shape"]["section"]["value"]["value"].as_str(),
        Some(sweep_section_feature_id),
        "Sweep profile feature"
    );
    let sweep_sections = native_property("Sweep", "Sections");
    let [Some(sweep_section)] = sweep_sections.links() else {
        panic!("Sweep.Sections link")
    };
    let sweep_section_native_id = crate::native::native_id("object", "SweepSection");
    assert_eq!(
        sweep_section.object(),
        Some(sweep_section_native_id.as_str())
    );
    let sweep_spine = native_property("Sweep", "Spine");
    assert_eq!(
        sweep["path"]["kind"].as_str(),
        Some("edges"),
        "Sweep path resolves to ordered neutral edges"
    );
    let [Some(sweep_path)] = sweep_spine.links() else {
        panic!("Sweep.Spine link")
    };
    let sweep_spine_id = crate::native::native_id("object", "SweepSpine");
    assert_eq!(
        sweep_path.object(),
        Some(sweep_spine_id.as_str()),
        "Sweep spine carrier"
    );
    assert_eq!(sweep_path.subelements(), ["Edge1", "Edge2"]);
    let sweep_spine_shape = native_property("SweepSpine", "Shape");
    let expected_spine_edges = [
        expected_topology_id(sweep_spine_shape, "Edge", 1),
        expected_topology_id(sweep_spine_shape, "Edge", 2),
    ];
    let sweep_feature = feature("Sweep");
    let FeatureOperation::Sweep {
        path: Some(PathRef::Edges(path_edges)),
        ..
    } = operation_definition(sweep_feature)
    else {
        panic!(
            "Sweep path resolves to ordered edges; decoded definition: {:#?}",
            sweep_feature.evaluation.definition()
        )
    };
    assert_eq!(
        path_edges
            .iter()
            .map(cadmpeg_ir::ids::EdgeId::as_str)
            .collect::<Vec<_>>(),
        expected_spine_edges.to_vec()
    );

    let mirror_source = native_property("Mirror", "Source");
    let [Some(mirror_source_link)] = mirror_source.links() else {
        panic!("Mirror.Source link")
    };
    let mirror_base_id = crate::native::native_id("object", "MirrorBase");
    assert_eq!(
        mirror_source_link.object(),
        Some(mirror_base_id.as_str()),
        "Mirror source carrier"
    );
    let mirror_feature = feature("Mirror");
    let FeatureOperation::MirrorShape { source, .. } = operation_definition(mirror_feature) else {
        panic!(
            "Mirror operation; decoded definition: {:#?}",
            mirror_feature.evaluation.definition()
        )
    };
    let BodySelection::Resolved { bodies, native } = source else {
        panic!("Mirror source resolves to a typed body selection")
    };
    assert_eq!(native, &mirror_source.id);
    assert_eq!(
        bodies.as_slice(),
        feature("MirrorBase").evaluation.outputs(),
        "Mirror selects the source feature output body"
    );

    let draft_base = native_property("Draft", "Base");
    let draft_base_id = crate::native::native_id("object", "DraftBase");
    assert_link(draft_base, draft_base_id.as_str(), &["Face1", "Face2"]);
    let draft_shape = native_property("DraftBase", "Shape");
    let expected_draft_faces = [
        expected_topology_id(draft_shape, "Face", 1),
        expected_topology_id(draft_shape, "Face", 2),
    ];
    let draft_feature = feature("Draft");
    let FeatureOperation::Draft { faces, anchor, .. } = operation_definition(draft_feature) else {
        panic!(
            "Draft operation; decoded definition: {:#?}",
            draft_feature.evaluation.definition()
        )
    };
    let FaceSelection::Resolved {
        faces: resolved_draft_faces,
        native,
    } = faces
    else {
        panic!("Draft.Base resolves to transferred faces")
    };
    assert_eq!(native, &draft_base.id);
    assert_eq!(
        resolved_draft_faces
            .iter()
            .map(cadmpeg_ir::ids::FaceId::as_str)
            .collect::<Vec<_>>(),
        expected_draft_faces
    );
    let draft_plane = native_property("Draft", "NeutralPlane");
    let draft_plane_id = crate::native::native_id("object", "DatumPlane");
    assert_link(draft_plane, draft_plane_id.as_str(), &[""]);
    let draft_pull = native_property("Draft", "PullDirection");
    let draft_axis_id = crate::native::native_id("object", "DatumAxis");
    assert_link(draft_pull, draft_axis_id.as_str(), &[""]);
    let cadmpeg_ir::features::DraftAnchor::NeutralPlane {
        plane: FaceSelection::Native(plane),
        pull: Some(pull),
    } = anchor
    else {
        panic!("Draft anchor retains native plane and resolved pull direction")
    };
    assert_eq!(plane, &draft_plane.id);
    for (actual, expected) in [
        (pull.direction.get().x, 0.0),
        (pull.direction.get().y, 0.0),
        (pull.direction.get().z, 1.0),
    ] {
        assert!((actual - expected).abs() < DIRECTION_TOLERANCE);
    }

    let thickness_faces = native_property("Thickness", "Faces");
    let thickness_base_id = crate::native::native_id("object", "ThicknessBase");
    assert_link(thickness_faces, thickness_base_id.as_str(), &["Face6"]);
    let thickness_shape = native_property("ThicknessBase", "Shape");
    let expected_thickness_face = expected_topology_id(thickness_shape, "Face", 6);
    let thickness_feature = feature("Thickness");
    let FeatureOperation::Shell {
        removed_faces: FaceSelection::Resolved { faces, native },
        ..
    } = operation_definition(thickness_feature)
    else {
        panic!(
            "Thickness face selection resolves with its native property; decoded definition: {:#?}",
            thickness_feature.evaluation.definition()
        )
    };
    assert_eq!(native, &thickness_faces.id);
    assert_eq!(faces.len(), 1);
    assert_eq!(faces[0].as_str(), expected_thickness_face);

    let hole_profile = native_property("Hole", "Profile");
    let hole_profile_native_id = crate::native::native_id("object", "HoleProfile");
    assert_link(hole_profile, hole_profile_native_id.as_str(), &[]);
    let hole_sketch = model
        .sketches
        .iter()
        .find(|sketch| sketch.name.as_deref() == Some("HoleProfile"))
        .unwrap();
    let hole_feature = feature("Hole");
    let FeatureOperation::Hole {
        profile: Some(PlanarProfileRef::Sketch(profile)),
        ..
    } = operation_definition(hole_feature)
    else {
        panic!(
            "Hole.Profile should resolve to authored sketch {:?}; decoded definition: {:#?}",
            hole_sketch.id,
            hole_feature.evaluation.definition()
        )
    };
    assert_eq!(profile, &hole_sketch.id);

    let pattern_originals = native_property("LinearPattern", "Originals");
    let pattern_base_id = crate::native::native_id("object", "PatternBase");
    assert_link(pattern_originals, pattern_base_id.as_str(), &[]);
    let pattern_feature = feature("LinearPattern");
    let FeatureOperation::Pattern { seeds, .. } = operation_definition(pattern_feature) else {
        panic!(
            "LinearPattern operation; decoded definition: {:#?}",
            pattern_feature.evaluation.definition()
        )
    };
    assert_eq!(
        seeds.as_slice(),
        &[PatternSeed::Feature(feature("PatternBase").id.clone())]
    );
    let pattern_direction = native_property("LinearPattern", "Direction");
    assert_link(pattern_direction, draft_axis_id.as_str(), &[""]);
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
    for (name, base, treatment) in [
        ("PartFillet", "DressupBase", [2.0, 2.0]),
        ("PartChamfer", "ChamferBase", [1.5, 2.5]),
    ] {
        let base_property = native_property(name, "Base");
        let base_id = crate::native::native_id("object", base);
        assert_link(base_property, base_id.as_str(), &[]);
        let selected_edges = native_property(name, "Edges");
        assert_part_edge(selected_edges, treatment);
        let (FeatureDefinition::Operation(op)
        | FeatureDefinition::PostProcess { operation: op, .. }) =
            feature(name).evaluation.definition();
        let selection = match op {
            FeatureOperation::Fillet { groups } => &groups.as_slice()[0].edges,
            FeatureOperation::Chamfer { groups, .. } => &groups.as_slice()[0].edges,
            _ => panic!("{name}: dress-up"),
        };
        let EdgeSelection::Resolved { edges, native } = selection else {
            panic!("{name}: source edge is unresolved")
        };
        assert_eq!(native, &base_property.id, "{name}: native base selector");
        let [edge] = edges.as_slice() else {
            panic!("{name}: expected one selected edge")
        };
        let source_shape = native_property(base, "Shape");
        assert_eq!(
            edge.as_str(),
            expected_topology_id(source_shape, "Edge", 1),
            "{name}: authored Edge1 selection"
        );
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
fn auxiliary_sweep_path_resolves_selected_native_edges() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use std::collections::BTreeSet;

    let bytes = include_bytes!("../../../../corpus/freecad_fcstd/fixtures/core_operations.FCStd");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = decoded.ir().native.namespace("fcstd").unwrap();
    let mut properties = native
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let entries = native
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let sweep_object_id = crate::native::native_id("object", "Sweep");
    let auxiliary_id = crate::native::native_id("property", "Sweep.AuxiliarySpine");
    let mut auxiliary = properties
        .iter()
        .find(|property| property.owner == sweep_object_id && property.name == "Spine")
        .expect("authored Sweep.Spine selector")
        .clone();
    auxiliary.id = auxiliary_id.clone();
    auxiliary.name = "AuxiliarySpine".to_owned();
    properties.push(auxiliary);

    let mut ir = decoded.ir().clone();
    let sweep = ir
        .model
        .features
        .iter_mut()
        .find(|feature| feature.native_ref.as_deref() == Some(sweep_object_id.as_str()))
        .expect("authored Sweep feature");
    sweep.evaluation.edit(|definition, _| {
        let operation = match definition {
            FeatureDefinition::Operation(operation)
            | FeatureDefinition::PostProcess { operation, .. } => operation,
        };
        let FeatureOperation::Sweep { orientation, .. } = operation else {
            panic!("authored Sweep definition: {operation:#?}")
        };
        *orientation = Some(cadmpeg_ir::features::SweepOrientation::Auxiliary {
            path: PathRef::Native(auxiliary_id.clone()),
            tangent: false,
            curvilinear: true,
        });
    });

    let occurrences = shape_occurrences(&decoded);
    let spine_object_id = crate::native::native_id("object", "SweepSpine");
    let shape_id = properties
        .iter()
        .find(|property| property.owner == spine_object_id && property.name == "Shape")
        .expect("authored spine shape property")
        .id
        .clone();
    let expected = [1, 2]
        .into_iter()
        .map(|index| {
            let id = occurrences
                .iter()
                .find(|occurrence| {
                    occurrence.property == shape_id
                        && occurrence.indexed_name == "Edge"
                        && occurrence.source_index == index
                })
                .expect("authored spine edge topology");
            cadmpeg_ir::ids::EdgeId::mint(id.topology_id.clone()).unwrap()
        })
        .collect::<Vec<_>>();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
    crate::design::resolve_operation_selections(
        &ctx,
        &mut ir,
        &properties,
        &entries,
        &occurrences,
        &BTreeSet::new(),
    )
    .unwrap();

    let sweep = ir
        .model
        .features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(sweep_object_id.as_str()))
        .unwrap();
    let FeatureOperation::Sweep {
        orientation:
            Some(cadmpeg_ir::features::SweepOrientation::Auxiliary {
                path: PathRef::Edges(edges),
                tangent: false,
                curvilinear: true,
            }),
        ..
    } = operation_definition(sweep)
    else {
        panic!(
            "auxiliary path resolves to ordered neutral edges; decoded definition: {:#?}",
            sweep.evaluation.definition()
        )
    };
    assert_eq!(edges.as_slice(), expected.as_slice());
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
            "Mirror",
            (pi * 3.0 * 3.0 * 10.0, 2.0 * pi * 3.0 * (3.0 + 10.0)),
            [-15.0, -3.0, 0.0, -9.0, 3.0, 10.0],
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
            "LinearPattern",
            (
                4.0 * 4.0 * 32.0,
                2.0 * (4.0 * 4.0 + 4.0 * 32.0 + 4.0 * 32.0),
            ),
            [0.0, 0.0, -24.0, 4.0, 4.0, 8.0],
        ),
        (
            "Loft",
            (pi * 4.0 * 4.0 * 14.0, 2.0 * pi * 4.0 * (4.0 + 14.0)),
            [-4.0, -4.0, 0.0, 4.0, 4.0, 14.0],
        ),
    ];
    let mut bodies = rows
        .iter()
        .map(|(name, _, _)| {
            let [id] = feature(name).evaluation.outputs().as_slice() else {
                panic!("{name}: expected one body")
            };
            id.clone()
        })
        .collect::<Vec<_>>();
    let [revolution_body] = feature("Revolution").evaluation.outputs().as_slice() else {
        panic!("Revolution: expected one surface body")
    };
    bodies.push(revolution_body.clone());
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
    for ((name, (expected_volume, expected_area), bounds), id) in rows.into_iter().zip(&bodies) {
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
    let revolution = report
        .bodies()
        .iter()
        .find(|body| body.body() == revolution_body.as_str())
        .expect("Revolution body measures");
    let cadmpeg_measure::value::Quantity::Unavailable(volume) =
        revolution.volume().as_ref().expect("Revolution volume")
    else {
        panic!("Revolution: an open surface has no volume")
    };
    assert_eq!(
        volume.reason(),
        cadmpeg_measure::reason::UnavailableReason::VolumeShellOpen
    );
    assert_eq!(
        volume.category(),
        cadmpeg_measure::reason::Category::Undefined
    );
    let surface_area = revolution.area().as_ref().unwrap().measured().unwrap();
    assert!(
        (surface_area.value().value.get() - 30.0 * pi).abs() < AREA_TOLERANCE,
        "Revolution: surface area"
    );
    let revolution_bbox = revolution.bbox().measured().unwrap().value();
    let revolution_x_min = 8.0 * (5.0 * pi / 8.0).cos();
    for (actual, expected) in revolution_bbox
        .min
        .coordinates()
        .into_iter()
        .chain(revolution_bbox.max.coordinates())
        .zip([revolution_x_min, 0.0, -8.0, 8.0, 0.0, 8.0])
    {
        assert!(
            (actual.get() - expected).abs() < BOX_TOLERANCE,
            "Revolution: bbox"
        );
    }
    ctx.finish_session().unwrap();
}
