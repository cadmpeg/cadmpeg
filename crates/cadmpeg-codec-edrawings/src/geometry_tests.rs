// SPDX-License-Identifier: Apache-2.0
//! Geometry decode contracts over synthetic packages: the part, the
//! assembly, and the streams that break a rule.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::products::{
    owner, sole_placement, Occurrence, ProductDefinition, ProductDefinitionKind,
};
use cadmpeg_ir::report::loss::{LossKind, LossNote};
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::topology::BodyKind;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::loss::EdrawingsLossCode;
use crate::test_support::hsf::{
    box_shells, matrix, translation, BodySpec, Item, ModelSpec, NodeSpec, Stream,
};
use crate::test_support::models::{
    annotation_stream, assembly, cube_part, solidworks_assembly, BOX_OFFSET, BOX_SIZE, CUBE_EDGE,
    CUBE_OFFSET, MM, SW_BOX_OFFSET, SW_BOX_SIZE, SW_DEEP_SEGMENT, SW_DEFAULT_HIDDEN_SEGMENT,
    SW_HIDDEN_SEGMENT, SW_HIDDEN_SUB_SEGMENT, SW_INHERITING_SEGMENT, SW_LISTED_SEGMENT,
};
use crate::test_support::{
    assembly_package, assembly_package_of, part_package, SOLIDWORKS_VERSION,
};
use crate::EdrawingsCodec;

fn parent_placement<'a>(ir: &'a CadIr, occurrence: &Occurrence) -> Option<&'a Occurrence> {
    let definition = owner(&ir.model.product_definitions, &occurrence.id).unwrap();
    sole_placement(&ir.model.occurrences, &definition.id)
}

fn decode(bytes: &[u8]) -> DecodeResult {
    EdrawingsCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

fn codes(result: &DecodeResult) -> Vec<LossKind> {
    result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.clone())
        .collect()
}

/// The first loss note of `code`.
fn note_of(result: &DecodeResult, code: EdrawingsLossCode) -> &LossNote {
    result
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == code.kind())
        .unwrap_or_else(|| panic!("{} is not charged", code.code()))
}

fn assert_valid(ir: &CadIr) {
    let report = cadmpeg_ir::validate_neutral(ir, Vec::new()).unwrap();
    assert!(report.is_ok(), "{report:?}");
}

/// The bounds of all points, millimetres.
fn bounds(ir: &CadIr) -> ([f64; 3], [f64; 3]) {
    let mut low = [f64::MAX; 3];
    let mut high = [f64::MIN; 3];
    for point in &ir.geometry.points {
        let Point3 { x, y, z } = point.position().get();
        for (axis, value) in [x, y, z].into_iter().enumerate() {
            low[axis] = low[axis].min(value);
            high[axis] = high[axis].max(value);
        }
    }
    (low, high)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-4
}

fn model_of(items: Vec<Item>, matrix: Option<[f32; 12]>) -> ModelSpec {
    ModelSpec {
        document: "d".into(),
        bodies: vec![BodySpec {
            colour: None,
            subs: vec![items],
        }],
        tree: vec![NodeSpec::Part {
            segment: "instance0".into(),
            name: "p".into(),
            body: 0,
            matrix,
            user_name: false,
        }],
        state: None,
    }
}

#[test]
fn a_cube_part_decodes_to_a_solid_with_six_faces() {
    let result = decode(&part_package(&cube_part().stream()));
    let ir = result.ir();

    assert!(result.report().geometry_transferred());
    assert_eq!(
        codes(&result),
        [EdrawingsLossCode::GeometryReconstructedFromMesh.kind()]
    );
    assert_eq!(ir.geometry.bodies.len(), 1);
    let body = ir.geometry.bodies.first().unwrap();
    assert_eq!(body.kind, BodyKind::Solid);
    assert_eq!(body.name.as_deref(), Some("synthetic cube"));
    let colour = body.color.unwrap();
    assert!(near(colour.r(), 200.0 / 255.0));
    assert_eq!(ir.geometry.faces.len(), 6);
    assert_eq!(ir.geometry.shells.len(), 1);

    let (low, high) = bounds(ir);
    assert!(low.iter().all(|v| near(*v, 0.0)), "{low:?}");
    assert!(high.iter().all(|v| near(*v, 10.0)), "{high:?}");

    assert_eq!(ir.model.product_definitions.len(), 1);
    let definition = ir.model.product_definitions.first().unwrap();
    assert!(definition.is_root());
    assert_eq!(definition.kind, Some(ProductDefinitionKind::Part));
    assert_eq!(definition.bodies.as_slice(), std::slice::from_ref(&body.id));
    assert_eq!(definition.source_name.as_deref(), Some("synthetic cube"));
    assert!(ir.model.occurrences.is_empty());
    assert_valid(ir);
}

#[test]
fn the_matrix_of_a_plain_part_moves_its_points() {
    let spec = model_of(
        box_shells([CUBE_EDGE; 3]),
        Some(matrix(
            [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            [0.1, 0.0, 0.0],
        )),
    );
    let result = decode(&part_package(&spec.stream()));
    let ir = result.ir();
    assert_eq!(ir.geometry.bodies.first().unwrap().kind, BodyKind::Solid);
    let (low, high) = bounds(ir);
    // x' = -y + 100, y' = x
    assert!(
        near(low[0], 90.0) && near(high[0], 100.0),
        "{low:?} {high:?}"
    );
    assert!(near(low[1], 0.0) && near(high[1], 10.0));
    assert_valid(ir);
}

#[test]
fn a_mirroring_matrix_keeps_the_solid_outward() {
    let mirror = matrix(
        [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        [0.0; 3],
    );
    let spec = model_of(box_shells([CUBE_EDGE; 3]), Some(mirror));
    let result = decode(&part_package(&spec.stream()));
    assert_eq!(
        result.ir().geometry.bodies.first().unwrap().kind,
        BodyKind::Solid
    );
    assert_valid(result.ir());
}

#[test]
fn an_assembly_decodes_to_definitions_occurrences_and_bodies() {
    let result = decode(&assembly_package(&assembly().stream()));
    let ir = result.ir();
    assert!(result.report().geometry_transferred());
    assert_eq!(
        codes(&result),
        [EdrawingsLossCode::GeometryReconstructedFromMesh.kind()]
    );
    assert_eq!(ir.geometry.bodies.len(), 2);
    assert!(ir.geometry.bodies.iter().all(|b| b.kind == BodyKind::Solid));
    assert_eq!(ir.geometry.faces.len(), 12);

    let kinds: Vec<_> = ir
        .model
        .product_definitions
        .iter()
        .map(|d| (d.kind, d.source_name.as_deref().unwrap_or("")))
        .collect();
    assert_eq!(
        kinds,
        [
            (Some(ProductDefinitionKind::Part), "body0"),
            (Some(ProductDefinitionKind::Part), "body1"),
            (
                Some(ProductDefinitionKind::Group),
                "synthetic assembly.sldasm"
            ),
            (Some(ProductDefinitionKind::Group), "Group A"),
            (Some(ProductDefinitionKind::Group), "Group B"),
        ]
    );
    let labels: Vec<_> = ir
        .model
        .product_definitions
        .iter()
        .take(2)
        .map(|d| d.label.as_deref())
        .collect();
    assert_eq!(labels, [Some("Cube-1"), Some("Box-1")]);

    let rows: Vec<_> = ir
        .model
        .occurrences
        .iter()
        .map(|o| {
            let owner = owner(&ir.model.product_definitions, &o.id).unwrap();
            let parent = ir
                .model
                .product_definitions
                .iter()
                .position(|d| d.id == owner.id);
            let definition = ir
                .model
                .product_definitions
                .iter()
                .position(|d| d.id == o.prototype);
            let ordinal = owner.occurrences.iter().position(|id| id == &o.id).unwrap();
            (
                o.name.as_deref().unwrap_or(""),
                parent,
                definition,
                ordinal,
                o.visible,
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("Cube-1", Some(2), Some(0), 0, None),
            ("Group A", Some(2), Some(3), 1, None),
            ("Group B", Some(3), Some(4), 0, None),
            ("Box-1", Some(4), Some(1), 0, None),
            ("Cube-2", Some(3), Some(0), 1, Some(false)),
        ]
    );
    assert_valid(ir);
}

#[test]
fn occurrence_transforms_compose_to_the_world_placement() {
    let result = decode(&assembly_package(&assembly().stream()));
    let ir = result.ir();
    let by_name = |name: &str| {
        ir.model
            .occurrences
            .iter()
            .find(|o| o.name.as_deref() == Some(name))
            .unwrap()
    };
    // The sub-assemblies place themselves at the identity.
    for name in ["Group A", "Group B"] {
        assert_eq!(
            by_name(name).transform.unwrap().affine_rows(),
            Transform::identity().affine_rows()
        );
    }
    // Box-1: x' = -y + 50, y' = x + 20, z' = z + 10 (millimetres).
    let rows = by_name("Box-1").transform.unwrap().affine_rows();
    let expected = [
        [0.0, -1.0, 0.0, f64::from(BOX_OFFSET[0] * MM)],
        [1.0, 0.0, 0.0, f64::from(BOX_OFFSET[1] * MM)],
        [0.0, 0.0, 1.0, f64::from(BOX_OFFSET[2] * MM)],
    ];
    for (row, want) in rows.iter().zip(expected) {
        for (got, want) in row.iter().zip(want) {
            assert!(near(*got, want), "{rows:?}");
        }
    }
    let cube2 = by_name("Cube-2").transform.unwrap().affine_rows();
    assert!(near(cube2[1][3], f64::from(CUBE_OFFSET[1] * MM)));
    // Composing the chain gives the world position of a box corner.
    let mut world = by_name("Box-1").transform.unwrap();
    let mut parent = by_name("Box-1");
    while let Some(placement) = parent_placement(ir, parent) {
        parent = placement;
        world = parent.transform.unwrap().compose(world).unwrap();
    }
    let corner = world
        .apply_point(Point3 {
            x: f64::from(BOX_SIZE[0] * MM),
            y: 0.0,
            z: 0.0,
        })
        .unwrap()
        .get();
    assert!(near(corner.x, 50.0) && near(corner.y, 40.0) && near(corner.z, 10.0));
}

#[test]
fn a_turned_shell_instance_closes_the_box_body() {
    let result = decode(&assembly_package(&assembly().stream()));
    let ir = result.ir();
    let box_body = ir
        .geometry
        .bodies
        .iter()
        .find(|b| b.name.as_deref() == Some("body1"))
        .unwrap();
    assert_eq!(box_body.kind, BodyKind::Solid);
    let (_, high) = bounds(ir);
    assert!(high[0] >= f64::from(BOX_SIZE[0] * MM) - 1e-4);
}

#[test]
fn visibility_comes_only_from_the_active_display_state() {
    let mut spec = assembly();
    spec.state = None;
    let result = decode(&assembly_package(&spec.stream()));
    assert!(result
        .ir()
        .model
        .occurrences
        .iter()
        .all(|o| o.visible.is_none()));
}

#[test]
fn an_open_body_becomes_a_sheet_with_a_loss() {
    let one_face: Vec<Item> = box_shells([CUBE_EDGE; 3]).into_iter().take(1).collect();
    let result = decode(&part_package(&model_of(one_face, None).stream()));
    assert!(result.report().geometry_transferred());
    assert_eq!(
        result.ir().geometry.bodies.first().unwrap().kind,
        BodyKind::Sheet
    );
    assert_eq!(
        codes(&result),
        [
            EdrawingsLossCode::GeometryReconstructedFromMesh.kind(),
            EdrawingsLossCode::GeometryOpenBody.kind()
        ]
    );
    assert_valid(result.ir());
}

#[test]
fn a_micrometre_plate_stays_a_solid_and_the_finer_weld_is_charged() {
    // 10 mm by 10 mm by 1 micrometre: as thin as the weld distance.
    let result = decode(&part_package(
        &model_of(box_shells([0.01, 0.01, 1.0e-6]), None).stream(),
    ));
    let ir = result.ir();
    assert_eq!(ir.geometry.bodies.first().unwrap().kind, BodyKind::Solid);
    assert_eq!(ir.geometry.faces.len(), 6);
    assert!(!codes(&result).contains(&EdrawingsLossCode::GeometryOpenBody.kind()));
    let note = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == EdrawingsLossCode::GeometryReconstructedFromMesh.kind())
        .unwrap();
    assert!(
        note.message.contains("1 bodies welded finer"),
        "{}",
        note.message
    );
    assert_valid(ir);
}

#[test]
fn an_inside_out_body_is_reversed_and_charged() {
    let mut items = box_shells([CUBE_EDGE; 3]);
    for item in &mut items {
        if let Item::Shell { points, .. } = item {
            points.swap(1, 3);
        }
    }
    let result = decode(&part_package(&model_of(items, None).stream()));
    assert_eq!(
        result.ir().geometry.bodies.first().unwrap().kind,
        BodyKind::Solid
    );
    assert!(codes(&result).contains(&EdrawingsLossCode::GeometryBodyReversed.kind()));
    assert!(!codes(&result).contains(&EdrawingsLossCode::GeometryOpenBody.kind()));
    assert_valid(result.ir());
}

#[test]
fn degenerate_triangles_are_dropped_and_charged() {
    let mut items = box_shells([CUBE_EDGE; 3]);
    items.push(Item::Shell {
        points: vec![[0.0; 3], [0.0; 3], [0.0; 3]],
        strips: vec![vec![0, 1, 2]],
    });
    let result = decode(&part_package(&model_of(items, None).stream()));
    assert!(codes(&result).contains(&EdrawingsLossCode::GeometryDegenerateTriangles.kind()));
    assert_eq!(
        result.ir().geometry.bodies.first().unwrap().kind,
        BodyKind::Solid
    );
}

#[test]
fn bad_shells_are_left_out_and_charged() {
    let mut items = box_shells([CUBE_EDGE; 3]);
    items.push(Item::Shell {
        points: vec![[0.0; 3]; 3],
        strips: vec![vec![0, 1, 7]],
    });
    let result = decode(&part_package(&model_of(items, None).stream()));
    assert!(!codes(&result).contains(&EdrawingsLossCode::ModelShellInvalid.kind()));
    let [given_up] = charges(&result)
        .try_into()
        .unwrap_or_else(|charges: Vec<String>| panic!("one record.unreadable: {charges:?}"));
    assert!(given_up.contains("model.shell-invalid"), "{given_up}");
    assert_eq!(result.ir().geometry.faces.len(), 6);
}

#[test]
fn a_pinned_decode_refuses_a_shell_that_does_not_decode() {
    let mut items = box_shells([CUBE_EDGE; 3]);
    items.push(Item::Shell {
        points: vec![[0.0; 3]; 3],
        strips: vec![vec![0, 1, 7]],
    });
    let package = part_package(&model_of(items, None).stream());
    let admitted = decode(&package);
    assert!(!codes(&admitted).contains(&EdrawingsLossCode::ModelShellInvalid.kind()));
    let [given_up] = charges(&admitted)
        .try_into()
        .unwrap_or_else(|charges: Vec<String>| panic!("one record.unreadable: {charges:?}"));
    assert!(
        given_up.starts_with("part model-object stream=\"eModel\" offset="),
        "{given_up}"
    );
    assert!(
        given_up.contains("edrawings/model.shell-invalid: strip index lies outside the shell"),
        "{given_up}"
    );
    assert!(given_up.contains("; dropped=[]"), "{given_up}");
    assert_eq!(admitted.ir().geometry.faces.len(), 6);

    let mut options = DecodeOptions::default();
    options.strictness.pinned = true;
    let failure = EdrawingsCodec
        .decode(&mut Cursor::new(package), &options)
        .expect_err("a pinned decode refuses a given-up shell");
    let cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection } = &failure else {
        panic!("expected a strict rejection, got {failure:?}");
    };
    assert_eq!(
        rejection.loss().code,
        cadmpeg_ir::unknown::part::record_unreadable_kind()
    );
    assert_eq!(rejection.loss().message, given_up);
}

/// The messages of the `record.unreadable` charges of `result`.
fn charges(result: &DecodeResult) -> Vec<String> {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind())
        .map(|loss| loss.message.clone())
        .collect()
}

#[test]
fn a_shell_that_does_not_decode_drops_its_body_and_occurrence() {
    let mut spec = assembly();
    spec.bodies[1] = BodySpec {
        colour: None,
        subs: vec![vec![Item::Shell {
            points: vec![[0.0; 3]; 3],
            strips: vec![vec![0, 1, 7]],
        }]],
    };
    let result = decode(&assembly_package(&spec.stream()));
    let [given_up] = charges(&result)
        .try_into()
        .unwrap_or_else(|charges: Vec<String>| panic!("one record.unreadable: {charges:?}"));
    assert!(given_up.starts_with("part model-object "), "{given_up}");
    let dropped = given_up
        .split_once("; dropped=[")
        .and_then(|(_, tail)| tail.split_once(']'))
        .map(|(list, _)| list.split(", ").collect::<Vec<_>>())
        .expect("the charge lists its dropped records");
    let [body, occurrence] = dropped.as_slice() else {
        panic!("a body and an occurrence: {dropped:?}");
    };
    assert!(body.starts_with("edrawings:model:body#"), "{body}");
    assert!(
        occurrence.starts_with("edrawings:model:occurrence#"),
        "{occurrence}"
    );
    let ir = result.ir();
    assert!(ir.geometry.bodies.iter().all(|b| b.id.to_string() != *body));
    assert!(ir
        .model
        .occurrences
        .iter()
        .all(|o| o.id.to_string() != *occurrence));
    assert_eq!(ir.model.occurrences.len(), 4);
}

#[test]
fn objects_outside_a_body_and_repeated_ids_give_up_their_own_records() {
    let mut s = Stream::new();
    s.shell_placeholder().slot();
    s.shell(0, &[[0.0; 3]; 3], &[vec![0, 1, 2]]);
    s.shell(0, &[[0.0; 3]; 3], &[vec![0, 1, 2]]);
    s.raw(b"z");
    let result = decode(&part_package(s.bytes()));
    let offset = |message: &str| -> u64 {
        let tail = message
            .strip_prefix("part model-object stream=\"eModel\" offset=")
            .unwrap_or_else(|| panic!("an object part: {message}"));
        tail.split_once(' ').unwrap().0.parse().unwrap()
    };
    let [orphan] = undecoded(&result)[..] else {
        panic!("one record.undecoded: {:?}", result.report().losses);
    };
    assert!(
        orphan.message.contains("outside every library body"),
        "{}",
        orphan.message
    );
    let [repeated] = charges(&result)
        .try_into()
        .unwrap_or_else(|charges: Vec<String>| panic!("one record.unreadable: {charges:?}"));
    assert!(
        repeated.contains("an earlier object has the id 0"),
        "{repeated}"
    );
    assert!(offset(&orphan.message) < offset(&repeated));
}

/// The `record.undecoded` charges of `result`.
fn undecoded(result: &DecodeResult) -> Vec<&LossNote> {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == cadmpeg_ir::unknown::part::record_undecoded_kind())
        .collect()
}

#[test]
fn a_pinned_decode_accepts_a_body_past_the_coordinate_bound() {
    let far = 1.0e9_f32;
    let items = vec![Item::Shell {
        points: vec![[far, 0.0, 0.0], [0.0; 3], [0.0, far, 0.0]],
        strips: vec![vec![0, 1, 2]],
    }];
    let package = part_package(&model_of(items, None).stream());
    let admitted = decode(&package);
    assert!(admitted.ir().geometry.bodies.is_empty());
    let [given_up] = undecoded(&admitted)[..] else {
        panic!("one record.undecoded: {:?}", admitted.report().losses);
    };
    assert!(given_up
        .message
        .split_whitespace()
        .any(|token| token.starts_with("edrawings:model:body#")));
    assert!(charges(&admitted).is_empty());

    let mut options = DecodeOptions::default();
    options.strictness.pinned = true;
    let pinned = EdrawingsCodec
        .decode(&mut Cursor::new(package), &options)
        .expect("a pinned decode accepts a body dropped at the coordinate bound");
    assert_eq!(undecoded(&pinned).len(), 1);
}

#[test]
fn a_body_whose_shells_hold_no_triangle_is_undecoded() {
    let items = vec![Item::Shell {
        points: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        strips: vec![vec![0, 1]],
    }];
    let result = decode(&part_package(&model_of(items, None).stream()));
    assert!(result.ir().geometry.bodies.is_empty());
    assert!(charges(&result).is_empty(), "{:?}", result.report().losses);
    let [given_up] = undecoded(&result)[..] else {
        panic!("one record.undecoded: {:?}", result.report().losses);
    };
    assert!(
        given_up.message.contains("mesh has no triangles"),
        "{}",
        given_up.message
    );
}

#[test]
fn an_instance_without_a_library_body_is_charged() {
    let mut s = Stream::new();
    s.open("includes")
        .open("assempart0")
        .open("part")
        .include("/include library/none/shared/body9");
    s.close().close().close().raw(b"z");
    let result = decode(&part_package(s.bytes()));
    assert!(!result.report().geometry_transferred());
    assert_eq!(
        codes(&result),
        [
            EdrawingsLossCode::ModelNoGeometry.kind(),
            cadmpeg_ir::unknown::part::record_undecoded_kind(),
        ]
    );
    let [given_up] = undecoded(&result)[..] else {
        panic!("one record.undecoded: {:?}", result.report().losses);
    };
    let given_up = &given_up.message;
    assert!(
        given_up.starts_with("part tree-node stream=\"eModel\" offset="),
        "{given_up}"
    );
    assert!(
        given_up.contains("edrawings/model.object-invalid: the instance names no library body"),
        "{given_up}"
    );
}

#[test]
fn broken_streams_give_up_the_model_member_with_offsets() {
    let mut bytes = cube_part().stream();
    bytes.push(0);
    let result = decode(&part_package(&bytes));
    let [note] = result.report().losses.as_slice() else {
        panic!("one charge: {:?}", result.report().losses);
    };
    assert_eq!(
        note.code,
        cadmpeg_ir::unknown::part::record_unreadable_kind()
    );
    assert_eq!(note.severity, Severity::Error);
    assert!(note.message.contains("24 byte trailer"), "{}", note.message);
    assert!(result.ir().geometry.bodies.is_empty());
    assert!(!result.report().geometry_transferred());

    let mut cut = cube_part().stream();
    cut.truncate(cut.len() / 2);
    let result = decode(&part_package(&cut));
    assert_eq!(
        codes(&result),
        [cadmpeg_ir::unknown::part::record_unreadable_kind()]
    );
}

#[test]
fn a_non_finite_plain_part_matrix_builds_no_body() {
    let spec = model_of(
        box_shells([CUBE_EDGE; 3]),
        Some(translation([f32::NAN, 0.0, 0.0])),
    );
    let result = decode(&part_package(&spec.stream()));
    assert!(!result.report().geometry_transferred());
    let [given_up] = charges(&result)
        .try_into()
        .unwrap_or_else(|charges: Vec<String>| panic!("one record.unreadable: {charges:?}"));
    assert!(given_up.contains("could not be built"), "{given_up}");
}

fn decode_solidworks(model: &crate::test_support::solidworks::SolidWorksModel) -> DecodeResult {
    decode(&assembly_package_of(SOLIDWORKS_VERSION, &model.stream()))
}

/// The occurrence with display name `name`.
fn occurrence_named<'a>(ir: &'a CadIr, name: &str) -> &'a cadmpeg_ir::products::Occurrence {
    ir.model
        .occurrences
        .iter()
        .find(|o| o.name.as_deref() == Some(name))
        .unwrap()
}

#[test]
fn a_late_solidworks_assembly_decodes_under_its_own_dialect() {
    let model = solidworks_assembly();
    let early = decode_solidworks(&model);
    let late = decode(&assembly_package_of("19.00", &model.stream()));
    let primary = late.ir().source.as_ref().unwrap().dialect().unwrap();
    assert_eq!(primary.dialect().as_str(), "edrawings:hsf-19-00");
    assert!(late.report().geometry_transferred());
    assert_eq!(codes(&late), codes(&early));
    assert_eq!(
        late.ir().model.occurrences.len(),
        early.ir().model.occurrences.len()
    );
}

#[test]
fn a_solidworks_assembly_decodes_under_its_own_dialect() {
    let result = decode_solidworks(&solidworks_assembly());
    let ir = result.ir();
    let primary = ir.source.as_ref().unwrap().dialect().unwrap();
    assert_eq!(primary.dialect().as_str(), "edrawings:hsf-18-00");
    assert!(result.report().geometry_transferred());
    assert_eq!(
        codes(&result),
        [
            EdrawingsLossCode::GeometryReconstructedFromMesh.kind(),
            EdrawingsLossCode::AppearanceFaceColourReduced.kind(),
            EdrawingsLossCode::AppearanceOverrideDropped.kind(),
            EdrawingsLossCode::StructureDisplayStatesDropped.kind(),
            EdrawingsLossCode::StructureVisibilityInferred.kind(),
        ]
    );
    let messages: Vec<&str> = result
        .report()
        .losses
        .iter()
        .map(|loss| loss.message.as_str())
        .collect();
    assert!(messages[1].starts_with("1 face group colours in 1 bodies"));
    assert!(messages[2].starts_with("1 part colour overrides and 0 display state"));
    // Three library bodies; two of them are also listed by a second body set,
    // whose definition holds a copy of each.
    assert_eq!(ir.geometry.bodies.len(), 5);
    assert!(ir.geometry.bodies.iter().all(|b| b.kind == BodyKind::Solid));
    assert_eq!(ir.geometry.faces.len(), 30);
    assert_valid(ir);
}

#[test]
fn body_colours_come_from_the_body_materials() {
    let result = decode_solidworks(&solidworks_assembly());
    let ir = result.ir();
    let colour = |name: &str| {
        ir.geometry
            .bodies
            .iter()
            .find(|b| b.name.as_deref() == Some(name))
            .and_then(|b| b.color)
            .unwrap()
    };
    assert!(near(colour("body0").r(), 202.0 / 255.0));
    assert!(near(colour("body1").b(), 1.0));
    assert!(near(colour("body2").g(), 1.0));
}

#[test]
fn a_definition_holds_its_own_copy_of_a_body_another_definition_lists() {
    let result = decode_solidworks(&solidworks_assembly());
    let ir = result.ir();
    let parts: Vec<_> = ir
        .model
        .product_definitions
        .iter()
        .filter(|d| d.kind == Some(ProductDefinitionKind::Part))
        .collect();
    assert_eq!(
        parts.iter().map(|d| d.bodies.len()).collect::<Vec<_>>(),
        [1, 1, 2, 1]
    );
    let listed: Vec<_> = ir
        .model
        .product_definitions
        .iter()
        .flat_map(|d| &d.bodies)
        .collect();
    assert_eq!(listed.len(), ir.geometry.bodies.len());
    assert!(listed
        .iter()
        .enumerate()
        .all(|(index, body)| !listed[..index].contains(body)));
    let body = |id: &BodyId| ir.geometry.bodies.iter().find(|b| &b.id == id).unwrap();
    let faces = |id: &BodyId| {
        ir.geometry
            .faces
            .iter()
            .filter(|face| {
                ir.geometry
                    .shells
                    .iter()
                    .find(|shell| shell.id == face.shell)
                    .and_then(|shell| ir.geometry.regions.iter().find(|r| r.id == shell.region))
                    .is_some_and(|region| &region.body == id)
            })
            .count()
    };
    let names = |definition: &ProductDefinition| {
        definition
            .bodies
            .iter()
            .map(|id| body(id).name.clone())
            .collect::<Vec<_>>()
    };
    let single = parts.first().unwrap();
    let pair = parts.get(2).unwrap();
    let cube = parts.get(3).unwrap();
    assert_eq!(names(single), [Some("body0".to_owned())]);
    assert_eq!(
        names(pair),
        [Some("body0".to_owned()), Some("body2".to_owned())]
    );
    assert_eq!(names(cube), [Some("body2".to_owned())]);
    for (copy, original) in [
        (&pair.bodies[0], &single.bodies[0]),
        (&cube.bodies[0], &pair.bodies[1]),
    ] {
        assert_ne!(copy, original);
        assert_eq!(body(copy).color, body(original).color);
        assert_eq!(body(copy).kind, body(original).kind);
        assert_eq!(faces(copy), 6);
        assert_eq!(faces(copy), faces(original));
    }
    assert!(pair.source_name.is_none());
    assert_eq!(pair.label.as_deref(), Some("Pair-1"));
    assert_eq!(single.source_name.as_deref(), Some("body0"));

    let prototype = |name: &str| occurrence_named(ir, name).prototype.clone();
    assert_eq!(prototype("Pair-1"), prototype("Pair-2"));
    assert_eq!(prototype("Cube-3"), prototype("Cube-4"));
    assert_ne!(prototype("Cube-1"), prototype("Cube-3"));
    // Four parts, the root, and three sub-assemblies.
    assert_eq!(ir.model.product_definitions.len(), 8);
    assert_eq!(ir.model.occurrences.len(), 9);
}

#[test]
fn nested_sub_assemblies_keep_their_names_and_the_identity_placement() {
    let result = decode_solidworks(&solidworks_assembly());
    let ir = result.ir();
    let parent_name =
        |name: &str| parent_placement(ir, occurrence_named(ir, name)).and_then(|o| o.name.clone());
    assert_eq!(parent_name("Group B").as_deref(), Some("Group A"));
    assert_eq!(parent_name("Box-1").as_deref(), Some("Group B"));
    assert_eq!(parent_name("Group A"), None);
    for name in ["Group A", "Group B", "Group C"] {
        assert_eq!(
            occurrence_named(ir, name).transform.unwrap().affine_rows(),
            Transform::identity().affine_rows()
        );
    }
    let group = ir
        .model
        .product_definitions
        .iter()
        .find(|d| d.source_name.as_deref() == Some("Group B"))
        .unwrap();
    assert_eq!(group.kind, Some(ProductDefinitionKind::Group));
    assert_eq!(group.native_ref.as_deref(), Some("subassem3"));
}

#[test]
fn a_leaf_two_sub_assemblies_deep_lands_exactly_at_its_matrix() {
    let result = decode_solidworks(&solidworks_assembly());
    let ir = result.ir();
    let leaf = occurrence_named(ir, "Box-1");
    assert_eq!(leaf.native_ref.as_deref(), Some(SW_DEEP_SEGMENT));
    let mut world = leaf.transform.unwrap();
    let mut current = leaf;
    let mut depth = 0;
    while let Some(placement) = parent_placement(ir, current) {
        current = placement;
        world = current.transform.unwrap().compose(world).unwrap();
        depth += 1;
    }
    assert_eq!(depth, 2);
    // p' = p * R + t with R = [[0,1,0],[-1,0,0],[0,0,1]] in the stream layout.
    let expected = [
        [0.0, -1.0, 0.0, f64::from(SW_BOX_OFFSET[0] * MM)],
        [1.0, 0.0, 0.0, f64::from(SW_BOX_OFFSET[1] * MM)],
        [0.0, 0.0, 1.0, f64::from(SW_BOX_OFFSET[2] * MM)],
    ];
    for (row, want) in world.affine_rows().iter().zip(expected) {
        for (got, want) in row.iter().zip(want) {
            assert!(near(*got, want), "{:?}", world.affine_rows());
        }
    }
    let corner = world
        .apply_point(Point3 {
            x: f64::from(SW_BOX_SIZE[0] * MM),
            y: 0.0,
            z: 0.0,
        })
        .unwrap()
        .get();
    assert!(near(corner.x, 50.0) && near(corner.y, 40.0) && near(corner.z, 10.0));
}

#[test]
fn a_shell_instance_may_name_a_source_in_another_body() {
    let result = decode_solidworks(&solidworks_assembly());
    let ir = result.ir();
    let body = ir
        .geometry
        .bodies
        .iter()
        .find(|b| b.name.as_deref() == Some("body1"))
        .unwrap();
    assert_eq!(body.kind, BodyKind::Solid);
    assert!(!codes(&result).contains(&EdrawingsLossCode::ModelShellInvalid.kind()));
}

#[test]
fn a_cross_body_source_that_is_missing_is_charged() {
    let mut model = solidworks_assembly();
    if let Some(crate::test_support::solidworks::SwItem::Foreign { source, .. }) = model
        .bodies
        .get_mut(1)
        .and_then(|b| b.subs.first_mut())
        .and_then(|s| s.items.get_mut(5))
    {
        *source = 99;
    }
    let result = decode_solidworks(&model);
    assert!(!codes(&result).contains(&EdrawingsLossCode::ModelShellInvalid.kind()));
    assert!(
        charges(&result)
            .iter()
            .any(|given_up| given_up
                .contains("model.shell-invalid: object is missing from the store"))
    );
    assert_eq!(
        result
            .ir()
            .geometry
            .bodies
            .iter()
            .find(|b| b.name.as_deref() == Some("body1"))
            .unwrap()
            .kind,
        BodyKind::Sheet
    );
}

#[test]
fn the_default_display_state_decides_visibility() {
    let result = decode_solidworks(&solidworks_assembly());
    let ir = result.ir();
    let visible = |name: &str| occurrence_named(ir, name).visible;
    // Listed `vis=on` under an unlisted sub-assembly.
    assert_eq!(visible("Pair-1"), Some(true));
    // A part that nothing lists and that sets `defaultvisibility=off`.
    assert_eq!(visible("Pair-2"), Some(false));
    // A sub-assembly with `vis=off` hides its parts.
    assert_eq!(visible("Group C"), Some(false));
    assert_eq!(visible("Cube-3"), Some(false));
    assert_eq!(visible("Cube-4"), Some(false));
    // Nothing lists these.
    assert_eq!(visible("Cube-1"), None);
    assert_eq!(visible("Group A"), None);
    assert_eq!(visible("Box-1"), None);
    for (segment, name) in [
        (SW_LISTED_SEGMENT, "Pair-1"),
        (SW_DEFAULT_HIDDEN_SEGMENT, "Pair-2"),
        (SW_HIDDEN_SUB_SEGMENT, "Group C"),
        (SW_INHERITING_SEGMENT, "Cube-3"),
        (SW_HIDDEN_SEGMENT, "Cube-4"),
    ] {
        assert_eq!(
            occurrence_named(ir, name).native_ref.as_deref(),
            Some(segment)
        );
    }
    // Pair-2 takes its value from `defaultvisibility=off` and Cube-3 from its
    // listed ancestor (EDR-14). The other values come from the listing of the
    // node itself and are not charged.
    let note = note_of(&result, EdrawingsLossCode::StructureVisibilityInferred);
    assert_eq!(note.severity, Severity::Warning);
    assert!(
        note.message.starts_with("2 occurrences"),
        "{}",
        note.message
    );
    assert!(note.message.contains("Pair-2") && note.message.contains("Cube-3"));
    for name in ["Pair-1", "Cube-4", "Group C"] {
        assert!(!note.message.contains(name), "{name}");
    }
}

#[test]
fn a_listing_against_a_hidden_ancestor_is_kept_and_charged() {
    let mut model = solidworks_assembly();
    model.states[0].components.push((
        format!("{SW_HIDDEN_SUB_SEGMENT}/{SW_INHERITING_SEGMENT}"),
        true,
    ));
    let result = decode_solidworks(&model);
    let ir = result.ir();
    // Cube-3 keeps its own `vis=on` listing under `vis=off` Group C (EDR-14).
    assert_eq!(occurrence_named(ir, "Group C").visible, Some(false));
    assert_eq!(occurrence_named(ir, "Cube-3").visible, Some(true));
    let note = note_of(&result, EdrawingsLossCode::StructureVisibilityInferred);
    assert!(note.message.contains("Cube-3"), "{}", note.message);
    // Cube-4 agrees with its listed ancestor and is not charged.
    assert!(!note.message.contains("Cube-4"), "{}", note.message);
}

#[test]
fn another_default_state_hides_a_whole_sub_assembly() {
    let mut model = solidworks_assembly();
    model.default_state = Some("disp1".into());
    let result = decode_solidworks(&model);
    let ir = result.ir();
    let visible = |name: &str| occurrence_named(ir, name).visible;
    for name in ["Group A", "Box-1", "Pair-1", "Pair-2"] {
        assert_eq!(visible(name), Some(false), "{name}");
    }
    // A sub-assembly shows its own listing only.
    for name in ["Group B", "Cube-1", "Group C", "Cube-3", "Cube-4"] {
        assert_eq!(visible(name), None, "{name}");
    }
}

#[test]
fn a_part_that_hides_by_default_stays_hidden_below_a_listed_sub_assembly() {
    let mut model = solidworks_assembly();
    model.default_state = Some("disp2".into());
    let result = decode_solidworks(&model);
    let ir = result.ir();
    let visible = |name: &str| occurrence_named(ir, name).visible;
    assert_eq!(visible("Group A"), Some(true));
    assert_eq!(visible("Box-1"), Some(true));
    assert_eq!(visible("Pair-1"), Some(true));
    assert_eq!(visible("Pair-2"), Some(false));
}

#[test]
fn without_a_default_state_only_the_default_visibility_applies() {
    let mut model = solidworks_assembly();
    model.default_state = None;
    let result = decode_solidworks(&model);
    let ir = result.ir();
    let hidden: Vec<_> = ir
        .model
        .occurrences
        .iter()
        .filter(|o| o.visible.is_some())
        .map(|o| o.name.as_deref().unwrap())
        .collect();
    assert_eq!(hidden, ["Pair-2"]);
}

#[test]
fn a_stream_of_the_other_grammar_under_the_solidworks_version_is_malformed() {
    let package = assembly_package_of(SOLIDWORKS_VERSION, &assembly().stream());
    let result = decode(&package);
    let note = result.report().losses.first().unwrap();
    assert_eq!(
        note.code,
        cadmpeg_ir::unknown::part::record_unreadable_kind()
    );
    assert!(note.message.contains("at byte 0"), "{}", note.message);
    assert!(!result.report().geometry_transferred());
}

#[test]
fn a_non_finite_assembly_placement_becomes_the_identity_and_is_charged() {
    let mut spec = assembly();
    if let Some(NodeSpec::Part { matrix, .. }) = spec.tree.first_mut() {
        *matrix = Some(translation([f32::NAN, 0.0, 0.0]));
    }
    let result = decode(&assembly_package(&spec.stream()));
    assert!(codes(&result).contains(&EdrawingsLossCode::StructurePlacementInvalid.kind()));
    let cube = result
        .ir()
        .model
        .occurrences
        .iter()
        .find(|o| o.name.as_deref() == Some("Cube-1"))
        .unwrap();
    assert_eq!(
        cube.transform.unwrap().affine_rows(),
        Transform::identity().affine_rows()
    );
}

/// A xorshift generator, so the mutations do not depend on a crate.
fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

#[test]
fn mutated_streams_never_panic() {
    const ROUNDS: usize = 1500;
    let original = assembly().stream();
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    for round in 0..ROUNDS {
        let mut bytes = original.clone();
        let edits = 1 + round % 4;
        for _ in 0..edits {
            let at =
                usize::try_from(next(&mut state) % u64::try_from(bytes.len()).unwrap()).unwrap();
            let value = u8::try_from(next(&mut state) & 0xff).unwrap();
            if let Some(byte) = bytes.get_mut(at) {
                *byte = value;
            }
        }
        if round % 7 == 0 {
            let keep =
                usize::try_from(next(&mut state) % u64::try_from(bytes.len()).unwrap()).unwrap();
            bytes.truncate(keep);
        }
        drop(EdrawingsCodec.decode(
            &mut Cursor::new(assembly_package(&bytes)),
            &DecodeOptions::default(),
        ));
    }
}

#[test]
fn dropped_closed_components_are_counted_apart_from_triangles() {
    let mut items = box_shells([CUBE_EDGE; 3]);
    items.push(Item::Shell {
        points: vec![[50.0, 0.0, 0.0], [51.0, 0.0, 0.0], [50.0, 1.0, 0.0]],
        strips: vec![vec![0, 1, 2, 0]],
    });
    let result = decode(&part_package(&model_of(items, None).stream()));
    let note = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == EdrawingsLossCode::GeometryDegenerateTriangles.kind())
        .expect("a drop note");
    assert_eq!(
        note.message,
        "1 closed components without a usable volume were dropped (1 zero-volume pieces; 2 triangles)"
    );
}

#[test]
fn geometry_and_image_side_objects_are_charged_as_annotation() {
    let result = decode(&part_package(&annotation_stream()));
    let note = note_of(&result, EdrawingsLossCode::AnnotationNotTransferred);
    assert_eq!(note.severity, Severity::Warning);
    // The stream holds two geometry side objects and no image object.
    assert!(note.message.starts_with("2 "), "{}", note.message);
    assert!(!codes(&decode(&part_package(&cube_part().stream())))
        .contains(&EdrawingsLossCode::AnnotationNotTransferred.kind()));

    let mut s = Stream::new();
    s.side_image(0, &[1, 2, 3]).close();
    let mut point = b"X".to_vec();
    point.extend_from_slice(&[0_u8; 12]);
    s.side_geometry(1, &point).close();
    s.side_empty(2).close();
    s.raw(b"z");
    let result = decode(&part_package(s.bytes()));
    let note = note_of(&result, EdrawingsLossCode::AnnotationNotTransferred);
    assert!(note.message.starts_with("2 "), "{}", note.message);
}

#[test]
fn a_matrix_on_a_sub_assembly_is_the_occurrence_transform_and_is_charged() {
    let mut spec = assembly();
    let shift = translation([0.001, 0.0, 0.0]);
    if let Some(NodeSpec::Sub { matrix, .. }) = spec.tree.get_mut(1) {
        *matrix = Some(shift);
    }
    let result = decode(&assembly_package(&spec.stream()));
    // The matrix holds 1 millimetre of translation along x.
    let rows = occurrence_named(result.ir(), "Group A")
        .transform
        .unwrap()
        .affine_rows();
    let expected = [
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    for (row, want) in rows.iter().zip(expected) {
        for (value, target) in row.iter().zip(want) {
            assert!(near(*value, target), "{rows:?}");
        }
    }
    let note = note_of(
        &result,
        EdrawingsLossCode::StructureSubassemblyPlacementAssumed,
    );
    assert_eq!(note.severity, Severity::Warning);
    assert!(
        note.message.starts_with("1 sub-assemblies"),
        "{}",
        note.message
    );
    assert!(note.message.contains("Group A") && !note.message.contains("Group B"));
    assert!(!codes(&decode(&assembly_package(&assembly().stream())))
        .contains(&EdrawingsLossCode::StructureSubassemblyPlacementAssumed.kind()));
}

#[test]
fn display_states_other_than_the_current_one_are_charged() {
    // The model names three states and applies `disp0`.
    let result = decode_solidworks(&solidworks_assembly());
    let note = note_of(&result, EdrawingsLossCode::StructureDisplayStatesDropped);
    assert_eq!(note.severity, Severity::Warning);
    assert!(
        note.message.starts_with("2 display states"),
        "{}",
        note.message
    );
    assert!(note.message.contains("disp1") && note.message.contains("disp2"));
    assert!(!note.message.contains("disp0"));
    // A model with one state, which is the current one, loses none.
    assert!(!codes(&decode(&assembly_package(&assembly().stream())))
        .contains(&EdrawingsLossCode::StructureDisplayStatesDropped.kind()));
}

#[test]
fn an_instance_whose_bodies_all_failed_charges_its_placement() {
    let mut spec = assembly();
    if let Some(body) = spec.bodies.get_mut(0) {
        body.subs = vec![vec![Item::Shell {
            points: vec![[0.0; 3]; 3],
            strips: vec![vec![0, 1, 7]],
        }]];
    }
    let result = decode(&assembly_package(&spec.stream()));
    let note = note_of(&result, EdrawingsLossCode::StructureOccurrenceDropped);
    assert_eq!(note.severity, Severity::Warning);
    assert!(note.message.starts_with("2 instances"), "{}", note.message);
    assert!(note.message.contains("Cube-1") && note.message.contains("Cube-2"));
    assert!(!note.message.contains("Box-1"));
    let ir = result.ir();
    assert!(ir
        .model
        .occurrences
        .iter()
        .all(|o| o.name.as_deref() != Some("Cube-1")));
    assert!(ir
        .model
        .occurrences
        .iter()
        .any(|o| o.name.as_deref() == Some("Box-1")));
}
