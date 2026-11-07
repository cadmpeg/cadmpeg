// SPDX-License-Identifier: Apache-2.0

use super::{components, identity, plans, SheetLine};
use crate::decode::ModelingGraphScope;
use crate::native::CatiaNative;
use cadmpeg_core::decode::View;
use cadmpeg_ir::math::Point2;

const CATALOG: &[&str] = &[
    "CATCatalogManager",
    "catalogManager",
    "catalogLinks",
    "",
    "GSMTool",
    "Open_body",
    "PRTSketch",
    "Sketch",
    "GSMFill",
    "SketchSupport",
    "2DPoint",
    "Point",
    "2DLine",
    "Line",
    "LENGTH",
    "Deviation",
    "Boundary",
    "Component",
    "DataBloc",
    "StartPoint",
    "EndPoint",
    "Sense",
    "MngFlgs",
    "ProjectedGeomSet",
    "R21ProjectedGeometrySet",
    "SolveManager",
    "AbsoluteAxis",
];

fn selector(name: &str) -> Vec<u8> {
    let index = CATALOG
        .iter()
        .position(|value| *value == name)
        .expect("fixture schema");
    let mut bytes = vec![0x32];
    bytes.extend(
        u32::try_from(index + 1)
            .expect("small catalog")
            .to_le_bytes(),
    );
    bytes
}

fn reference(value: u32, wide: bool) -> Vec<u8> {
    if wide {
        let mut bytes = vec![0x80];
        bytes.extend(value.to_le_bytes());
        bytes
    } else {
        vec![u8::try_from(value + 0x81).expect("short reference")]
    }
}

fn key(value: u32) -> u32 {
    0x1122_0000 + value
}

fn frame(marker: u8, body: Vec<u8>) -> Vec<u8> {
    let mut bytes = vec![0x7c, marker];
    bytes.extend(
        u32::try_from(body.len() + 6)
            .expect("fixture frame")
            .to_le_bytes(),
    );
    bytes.extend(body);
    bytes
}

fn scalar(bytes: &mut Vec<u8>, value: f64) {
    if value.to_bits() == 0.0_f64.to_bits() {
        bytes.push(0xe8);
    } else if value.to_bits() == 1.0_f64.to_bits() {
        bytes.push(0xe7);
    } else if value.to_bits() == (-1.0_f64).to_bits() {
        bytes.push(0xe9);
    } else {
        bytes.push(0xe6);
        bytes.extend(value.to_le_bytes());
    }
}

fn geometry(values: &[f64], layout: u8, value_type: u16) -> Vec<u8> {
    let mut bytes = selector("DataBloc");
    bytes.extend([0x81, 0xe8]);
    bytes.extend(value_type.to_le_bytes());
    bytes.extend([0x37, 0x80 + layout, 0x81]);
    for value in values {
        scalar(&mut bytes, *value);
    }
    bytes.push(0xfe);
    bytes
}

fn line_value(values: [f64; 7], sense: Option<i32>, flags: Option<u8>) -> Vec<u8> {
    let mut bytes = Vec::new();
    if let Some(flags) = flags {
        bytes.extend(selector("MngFlgs"));
        bytes.extend([0x81, 0xe8, 0xe0, 0x0a, 0x37, 0x82, 0x81, flags, 0xfe]);
    }
    bytes.extend(geometry(&values, 8, 0x07e4));
    if let Some(sense) = sense {
        bytes.extend(selector("Sense"));
        bytes.extend([0x81, 0xe8, 0xc0, 0x1c, 0x37, 0x80]);
        bytes.extend(sense.to_le_bytes());
        bytes.push(0xfe);
    }
    bytes.push(0xfe);
    bytes
}

struct Item {
    names: [&'static str; 2],
    owner: u32,
    payload: Vec<u8>,
    value: Vec<u8>,
}

fn endpoints(start: u32, end: u32, wide: bool) -> Vec<u8> {
    let mut bytes = selector("StartPoint");
    bytes.extend(reference(start, wide));
    bytes.extend(selector("EndPoint"));
    bytes.extend(reference(end, wide));
    bytes.push(0xfe);
    bytes
}

#[derive(Clone, Copy)]
enum FixtureCase {
    Valid,
    BoundaryKeyMismatch,
    RuntimeClassMismatch,
    ActiveProjection,
}

#[derive(Clone, Copy)]
struct FixtureOptions {
    wide: bool,
    case: FixtureCase,
    point_error: f64,
    axis_id: u32,
}

impl Default for FixtureOptions {
    fn default() -> Self {
        Self {
            wide: false,
            case: FixtureCase::Valid,
            point_error: 0.0,
            axis_id: 4,
        }
    }
}

fn fixture(options: FixtureOptions) -> Vec<u8> {
    let FixtureOptions {
        wide,
        case,
        point_error,
        axis_id,
    } = options;
    let wrong_key = matches!(case, FixtureCase::BoundaryKeyMismatch);
    let wrong_class = matches!(case, FixtureCase::RuntimeClassMismatch);
    let active_projection = matches!(case, FixtureCase::ActiveProjection);
    let mut parent = vec![0x3b, 0x83, 0x81];
    for id in [2, 3] {
        parent.extend(reference(id, wide));
        parent.push(0x80);
        parent.extend(key(id).to_le_bytes());
    }
    parent.push(0xfe);
    let mut roster = vec![0x3b, if active_projection { 0x8d } else { 0x8c }];
    for id in [4, 5, 6, 7, 8, 9, 10, 12, 13, 14, 15]
        .into_iter()
        .chain(active_projection.then_some(16))
    {
        roster.push(0x81);
        roster.extend(reference(id, wide));
    }
    roster.push(0x81);
    roster.extend(selector("SolveManager"));
    roster.extend(reference(15, wide));
    roster.extend(selector("AbsoluteAxis"));
    roster.extend(reference(axis_id, wide));
    roster.push(0xfe);
    let mut fill = selector("Boundary");
    fill.extend([0x82, 0x81]);
    fill.extend(reference(2, wide));
    fill.push(0x80);
    fill.extend(key(if wrong_key { 7 } else { 2 }).to_le_bytes());
    fill.extend(selector("Deviation"));
    fill.extend(reference(11, wide));
    fill.extend(selector("Component"));
    fill.extend([0x82, 0x81]);
    fill.extend(reference(11, wide));
    fill.extend([0x81, 0xfe]);
    let point = |u, v| Item {
        names: ["2DPoint", "Point"],
        owner: 2,
        payload: vec![0xfe],
        value: geometry(&[u, v], 3, 0x07e4),
    };
    let mut items = vec![
        Item {
            names: ["GSMTool", "Open_body"],
            owner: 0,
            payload: parent,
            value: vec![0xfe],
        },
        Item {
            names: ["PRTSketch", "Sketch"],
            owner: 1,
            payload: roster,
            value: vec![0xfe],
        },
        Item {
            names: ["GSMFill", "GSMFill"],
            owner: 1,
            payload: fill,
            value: vec![0xfe],
        },
        Item {
            names: ["SketchSupport", "SketchSupport"],
            owner: 2,
            payload: vec![0xfe],
            value: geometry(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 2.0], 10, 0x07e0),
        },
        point(point_error, 0.0),
        point(3.0, 0.0),
        point(0.0, 4.0),
        Item {
            names: ["2DLine", "Line"],
            owner: 2,
            payload: endpoints(5, 6, wide),
            value: line_value([0.0, 0.0, 1.0, 0.0, 0.0, 3.0, 1.0], Some(1), None),
        },
        Item {
            names: ["2DLine", "Line"],
            owner: 2,
            payload: endpoints(6, 7, wide),
            value: line_value([3.0, 0.0, -0.6, 0.8, 0.0, 5.0, 1.0], Some(1), None),
        },
        Item {
            names: ["2DLine", "Line"],
            owner: 2,
            payload: endpoints(5, 7, wide),
            value: line_value([0.0, 0.0, 0.0, 1.0, 0.0, 4.0, 1.0], Some(-1), None),
        },
        Item {
            names: ["LENGTH", "Deviation"],
            owner: 3,
            payload: vec![0xfe],
            value: vec![0xfe],
        },
        point(10.0, 10.0),
        point(12.0, 10.0),
        Item {
            names: ["2DLine", "Line"],
            owner: 2,
            payload: endpoints(12, 13, wide),
            value: line_value([10.0, 10.0, 1.0, 0.0, 0.0, 2.0, 1.0], None, Some(2)),
        },
        Item {
            names: ["SolveManager", "SolveManager"],
            owner: 2,
            payload: vec![0xfe],
            value: vec![0xfe],
        },
    ];
    if active_projection {
        items.push(Item {
            names: ["R21ProjectedGeometrySet", "ProjectedGeomSet"],
            owner: 2,
            payload: vec![0xfe],
            value: vec![0xfe],
        });
    } else if axis_id == 16 {
        items.push(Item {
            names: ["SketchSupport", "SketchSupport"],
            owner: 2,
            payload: vec![0xfe],
            value: geometry(
                &[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1000.0, 0.0, 2.0],
                10,
                0x07e0,
            ),
        });
    }
    let mut entities = Vec::new();
    let mut objects = Vec::new();
    for (index, item) in items.into_iter().enumerate() {
        let id = u32::try_from(index + 1).expect("small identity");
        let class = u8::try_from(index + 1).expect("small class");
        let mut definition = vec![0, 8];
        definition.extend(selector(item.names[0]));
        definition.extend(selector(item.names[1]));
        definition.push(0xea);
        definition.extend(id.to_le_bytes());
        for word in [key(id), 0, 0]
            .into_iter()
            .take(if id == 11 { 2 } else { 3 })
        {
            definition.push(0x80);
            definition.extend(word.to_le_bytes());
        }
        if id == 11 {
            definition.push(0x81);
        }
        definition.extend([3, 0x80 + class]);
        let mut entity = vec![0];
        entity.extend(frame(6, definition));
        entity.extend(frame(7, item.value));
        entities.extend(frame(5, entity));
        let mut object = vec![
            0x14,
            1,
            0x80 + if wrong_class && id == 8 {
                class + 1
            } else {
                class
            },
        ];
        object.extend(reference(item.owner, wide));
        object.extend(frame(0x0a, item.payload));
        objects.extend(frame(9, object));
    }
    entities.push(0xde);
    entities.extend(frame(8, objects));
    let mut catalog = vec![0x80 + u8::try_from(CATALOG.len() + 1).expect("short catalog")];
    for name in CATALOG {
        catalog.push(u8::try_from(name.len() + 1).expect("short name"));
        catalog.extend(name.as_bytes());
    }
    entities.extend(frame(2, catalog));
    entities
}

#[test]
fn fill_binds_source_keys_members_endpoints_and_reversed_line_without_changing_native() {
    for wide in [false, true] {
        let bytes = fixture(FixtureOptions {
            wide,
            ..FixtureOptions::default()
        });
        let native = CatiaNative::decode_with_record_ranges(&bytes, &[]);
        let before = native.clone();
        crate::test_support::with_service_context(|ctx| {
            let result = plans(
                ctx,
                View::over_retained(&bytes),
                &native,
                &ModelingGraphScope::Unscoped,
                1.0e-6,
            )
            .expect("bounded source binding");
            assert_eq!(result.len(), 1);
            let plan = &result[0];
            assert!((plan.origin.z - 2.0).abs() < 1.0e-12);
            assert_eq!(plan.components.len(), 1);
            assert_eq!(plan.components[0].len(), 3);
            assert_eq!(
                plan.components[0]
                    .iter()
                    .map(|line| (line.start_id, line.end_id, line.forward))
                    .collect::<Vec<_>>(),
                [(5, 6, true), (6, 7, true), (5, 7, false)]
            );
        });
        assert_eq!(native, before);
        assert!(native
            .entity_records
            .iter()
            .any(|entity| entity.entity_id == 14));
    }
}

#[test]
fn source_mismatch_unknown_geometry_and_scope_withhold_the_plan() {
    for case in [
        FixtureCase::BoundaryKeyMismatch,
        FixtureCase::RuntimeClassMismatch,
        FixtureCase::ActiveProjection,
    ] {
        let bytes = fixture(FixtureOptions {
            case,
            ..FixtureOptions::default()
        });
        let native = CatiaNative::decode_with_record_ranges(&bytes, &[]);
        crate::test_support::with_service_context(|ctx| {
            assert!(plans(
                ctx,
                View::over_retained(&bytes),
                &native,
                &ModelingGraphScope::Unscoped,
                1.0e-6,
            )
            .expect("bounded source rejection")
            .is_empty());
        });
    }
    let bytes = fixture(FixtureOptions::default());
    let mut native = CatiaNative::decode_with_record_ranges(&bytes, &[]);
    crate::test_support::with_service_context(|ctx| {
        assert!(plans(
            ctx,
            View::over_retained(&bytes),
            &native,
            &ModelingGraphScope::Unresolved,
            1.0e-6,
        )
        .expect("unresolved scope")
        .is_empty());
    });
    let duplicate = native
        .entity_records
        .iter()
        .find(|entity| entity.entity_id == 8)
        .expect("line entity")
        .clone();
    native.entity_records.push(duplicate);
    crate::test_support::with_service_context(|ctx| {
        assert!(plans(
            ctx,
            View::over_retained(&bytes),
            &native,
            &ModelingGraphScope::Unscoped,
            1.0e-6,
        )
        .expect("duplicate source identity")
        .is_empty());
    });
}

#[test]
fn graph_without_fill_uses_no_retained_memory_allowance() {
    let mut bytes = fixture(FixtureOptions::default());
    let at = bytes
        .windows(7)
        .position(|word| word == b"GSMFill")
        .expect("source class name");
    bytes[at..at + 7].copy_from_slice(b"GSMJoin");
    let native = CatiaNative::decode_with_record_ranges(&bytes, &[]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, source) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("input fits the independent input limit");
    assert!(
        plans(&ctx, source, &native, &ModelingGraphScope::Unscoped, 1.0e-6)
            .expect("unrelated graph needs no sheet allocation")
            .is_empty()
    );
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn tighter_model_tolerance_withholds_a_source_fitted_sheet() {
    let bytes = fixture(FixtureOptions {
        point_error: 5.0e-7,
        ..FixtureOptions::default()
    });
    let native = CatiaNative::decode_with_record_ranges(&bytes, &[]);
    for (linear_tolerance, expected_plans) in [(1.0e-6, 1), (1.0e-8, 0)] {
        crate::test_support::with_service_context(|ctx| {
            let result = plans(
                ctx,
                View::over_retained(&bytes),
                &native,
                &ModelingGraphScope::Unscoped,
                linear_tolerance,
            )
            .expect("bounded tolerance admission");
            assert_eq!(result.len(), expected_plans);
        });
    }
}

#[test]
fn absolute_axis_rejects_wrong_member_class_and_unlisted_support_decoys() {
    for axis_id in [5, 16] {
        let bytes = fixture(FixtureOptions {
            axis_id,
            ..FixtureOptions::default()
        });
        let native = CatiaNative::decode_with_record_ranges(&bytes, &[]);
        crate::test_support::with_service_context(|ctx| {
            assert!(plans(
                ctx,
                View::over_retained(&bytes),
                &native,
                &ModelingGraphScope::Unscoped,
                1.0e-6
            )
            .expect("source-selected axis check")
            .is_empty());
        });
    }
}

#[test]
fn reference_forms_select_the_same_identity_in_their_source_encoding() {
    for bytes in [
        vec![0x83],
        vec![0xd1, 2],
        vec![0x80, 2, 0, 0, 0],
        vec![0x32, 2, 0, 0, 0],
    ] {
        let mut view = View::over_retained(&bytes);
        assert_eq!(identity(&mut view), Some(2));
        assert!(view.is_empty());
    }
    for bytes in [vec![0x80, 2, 0], vec![0xd1], vec![0x32, 2, 0]] {
        assert!(identity(&mut View::over_retained(&bytes)).is_none());
    }
}

#[test]
fn primitive_short_third_word_keeps_its_key_and_ambiguous_suffix_is_rejected() {
    let bytes = fixture(FixtureOptions::default());
    let native = CatiaNative::decode_with_record_ranges(&bytes, &[]);
    let mut parameter = native
        .entity_records
        .iter()
        .find(|entity| entity.entity_id == 11)
        .expect("primitive Deviation metadata")
        .clone();
    assert_eq!(super::definition_identity(&parameter), Some((11, key(11))));
    let crate::native::entity_record::CatiaEntityRecordBody::Nested {
        definition_suffix, ..
    } = &mut parameter.body
    else {
        panic!("nested primitive fixture");
    };
    definition_suffix.clear();
    for word in [0x8000_0001_u32, 0x8000_0002, 0x8100_0003] {
        definition_suffix.push(0x80);
        definition_suffix.extend(word.to_le_bytes());
    }
    definition_suffix.extend([3, 0x8b]);
    assert!(super::definition_identity(&parameter).is_none());
}

fn polygon(points: &[(f64, f64)], base: u32) -> Vec<Option<SheetLine>> {
    points
        .iter()
        .enumerate()
        .map(|(index, &(u, v))| {
            let (eu, ev) = points[(index + 1) % points.len()];
            let start_id = base + u32::try_from(index).expect("small vertex");
            let end_id = base + u32::try_from((index + 1) % points.len()).expect("small vertex");
            Some(SheetLine {
                source_offset: u64::from(start_id),
                start_id,
                end_id,
                start: Point2::new(u, v),
                end: Point2::new(eu, ev),
                origin: Point2::new(u, v),
                direction: Point2::new(1.0, 0.0),
                interval: [0.0, 1.0],
                forward: true,
            })
        })
        .collect()
}

#[test]
fn disjoint_components_survive_source_order_permutation() {
    let mut lines = polygon(&[(0.0, 0.0), (2.0, 0.0), (0.0, 2.0)], 1);
    lines.extend(polygon(&[(5.0, 0.0), (7.0, 0.0), (5.0, 2.0)], 10));
    lines.swap(0, 4);
    crate::test_support::with_service_context(|ctx| {
        let result = components(ctx, &lines).expect("bounded component selection");
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|component| component.len() == 3));
    });
}

#[test]
fn nested_crossing_touching_and_self_intersecting_components_are_ambiguous() {
    let outer = polygon(&[(0.0, 0.0), (8.0, 0.0), (8.0, 8.0), (0.0, 8.0)], 1);
    for second in [
        vec![(2.0, 2.0), (4.0, 2.0), (2.0, 4.0)],
        vec![(6.0, 6.0), (10.0, 6.0), (6.0, 10.0)],
        vec![(8.0, 0.0), (10.0, 0.0), (8.0, 2.0)],
    ] {
        let mut lines = outer.clone();
        lines.extend(polygon(&second, 20));
        crate::test_support::with_service_context(|ctx| {
            assert!(components(ctx, &lines)
                .expect("bounded ambiguity check")
                .is_empty());
        });
    }
    let crossing = polygon(&[(0.0, 0.0), (3.0, 3.0), (0.0, 3.0), (2.0, 0.0)], 1);
    crate::test_support::with_service_context(|ctx| {
        assert!(components(ctx, &crossing)
            .expect("self intersection check")
            .is_empty());
    });
}

#[test]
fn open_and_branched_contours_do_not_invent_closure() {
    let mut open = polygon(&[(0.0, 0.0), (2.0, 0.0), (0.0, 2.0)], 1);
    open.pop();
    crate::test_support::with_service_context(|ctx| {
        assert!(components(ctx, &open).expect("open component").is_empty());
    });
    let mut branch = polygon(&[(0.0, 0.0), (2.0, 0.0), (0.0, 2.0)], 1);
    let mut extra = branch[0].expect("fixture line");
    extra.source_offset = 50;
    extra.end_id = 50;
    extra.end = Point2::new(-2.0, 0.0);
    branch.push(Some(extra));
    crate::test_support::with_service_context(|ctx| {
        assert!(components(ctx, &branch)
            .expect("branched component")
            .is_empty());
    });
}
