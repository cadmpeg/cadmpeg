// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};

use super::{
    finalize_unread_marker_entities, project_marker_backed_sketches, ChordConflict,
    HeldMarkerGeometry,
};
use crate::loss::SldprtLossCode;
use crate::records::{
    Feature as NativeFeature, FeatureHistory, FeatureInputLane, FeatureSource, SketchInputEntity,
    SketchInputKind, SketchInputLink, SketchInputLinks,
};

fn input_marker(
    id: &str,
    ordinal: u32,
    kind: SketchInputKind,
    coordinates_m: Option<[f64; 2]>,
) -> SketchInputEntity {
    marker_at(id, ordinal, 0, kind, coordinates_m)
}

fn marker_at(
    id: &str,
    ordinal: u32,
    offset: u64,
    kind: SketchInputKind,
    coordinates_m: Option<[f64; 2]>,
) -> SketchInputEntity {
    let mut marker = SketchInputEntity::new(id, "lane", ordinal, offset, kind);
    marker.feature_ref = Some("feature-native".into());
    marker.state_value = None;
    marker.coordinates_m = coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
    marker.links = None;
    marker
}

fn native_kind(entity: &SketchEntity) -> Option<&str> {
    match entity.geometry.definition() {
        SketchGeometryDefinition::Native { native_kind } => Some(native_kind.as_str()),
        _ => None,
    }
}

fn entity(id: &str, geometry: SketchGeometry) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:id#entity-{id}")).unwrap(),
        SketchId::mint("synthetic:test:id#sketch").unwrap(),
        geometry,
    )
    .with_native_ref(Some(format!("marker-{id}")))
}

fn placeholder(kind: &str) -> SketchGeometry {
    SketchGeometry::native(cadmpeg_core::text::NonBlankString::new(kind).unwrap())
}

/// The entities and held marker geometry `project_marker_backed_sketches` emits
/// for `markers` of one sketch feature whose lane holds `payload`.
fn projected_entities(
    markers: Vec<SketchInputEntity>,
    payload: Vec<u8>,
) -> (Vec<SketchEntity>, HeldMarkerGeometry) {
    let histories = vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![NativeFeature {
            id: "feature-native".into(),
            parent: "history".into(),
            xml_tag: "Sketch".into(),
            tree_parent: None,
            source_id: FeatureSource::from_value(1),
            ordinal: 1,
            name: "generated-profile".into(),
            kind: String::new(),
            input_class: None,
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }],
    }];
    let mut features = vec![Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:id#feature").expect("identity grammar"),
        ordinal: 0,
        name: Some("generated-profile".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
            }),
        ),
        native_ref: Some("feature-native".into()),
    }];
    let lanes = vec![FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload.into(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: markers,
    }];
    let mut sketches = Vec::new();
    let mut entities = Vec::new();
    let mut held = HeldMarkerGeometry::default();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("marker profile fixture fits service policy");
    project_marker_backed_sketches(
        &ctx,
        &mut features,
        &mut sketches,
        &mut entities,
        &mut held,
        &histories,
        &lanes,
    )
    .expect("marker profile projection fits service policy");
    (entities, held)
}

#[test]
fn a_line_or_circle_marker_without_coordinates_keeps_its_role_and_identity() {
    let (mut entities, held) = projected_entities(
        vec![
            input_marker("point", 0, SketchInputKind::Point, Some([0.001, 0.002])),
            input_marker("curve", 1, SketchInputKind::LineOrCircle, None),
        ],
        vec![0],
    );
    let curve = entities
        .iter()
        .position(|entity| entity.native_ref.as_deref() == Some("curve"))
        .expect("the unread curve marker is projected");
    let ids_before = entities
        .iter()
        .map(|entity| entity.id().clone())
        .collect::<Vec<_>>();

    let mut losses = Vec::new();
    finalize_unread_marker_entities(&[], &mut entities, held, &mut losses);

    assert_eq!(
        entities
            .iter()
            .map(|entity| entity.id().clone())
            .collect::<Vec<_>>(),
        ids_before,
        "relations reference entities by id, so every id survives"
    );
    assert!(entities
        .iter()
        .filter_map(native_kind)
        .all(|kind| !kind.starts_with("sldprt:marker-geometry:")));
    assert_eq!(
        native_kind(&entities[curve]),
        Some("sldprt:line-or-circle-handle")
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        SldprtLossCode::SketchHandleUnresolved.kind()
    );
    assert!(losses[0].message.contains(entities[curve].id().as_str()));
    assert!(losses[0].message.contains("line-or-circle handle curve"));
}

#[test]
fn each_placeholder_takes_its_marker_role_and_resolved_geometry_is_untouched() {
    let line = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(1.0, 0.0),
    })
    .unwrap();
    let source_native = placeholder("synthetic:curve");
    let mut entities = vec![
        entity("line", line.clone()),
        entity("source-native", source_native.clone()),
        entity(
            "unread-line-or-circle",
            placeholder("sldprt:marker-geometry:1"),
        ),
        entity("unread-arc", placeholder("sldprt:marker-geometry:2")),
        entity(
            "degenerate",
            placeholder("sldprt:marker-geometry:degenerate-line"),
        ),
    ];
    let mut losses = Vec::new();
    finalize_unread_marker_entities(
        &[],
        &mut entities,
        HeldMarkerGeometry::default(),
        &mut losses,
    );

    assert_eq!(entities[0].geometry, line);
    assert_eq!(entities[1].geometry, source_native);
    assert_eq!(
        native_kind(&entities[2]),
        Some("sldprt:line-or-circle-handle")
    );
    assert_eq!(native_kind(&entities[3]), Some("sldprt:arc-handle"));
    assert_eq!(
        native_kind(&entities[4]),
        Some("sldprt:line-or-circle-handle")
    );
    assert_eq!(
        losses
            .iter()
            .map(|loss| loss.code.clone())
            .collect::<Vec<_>>(),
        vec![
            SldprtLossCode::SketchHandleUnresolved.kind(),
            SldprtLossCode::SketchHandleUnresolved.kind(),
            SldprtLossCode::SketchLineDegenerate.kind(),
        ]
    );
    assert!(losses[2].message.contains("marker-degenerate coincide"));
}

/// A geometry-locus arc record with its center, start and end stored inline
/// (the direct-center layout), coordinates in metres.
fn inline_arc_record(center: [f64; 2], start: [f64; 2], end: [f64; 2]) -> Vec<u8> {
    let prefix = super::super::LEGACY_EXTENDED_SKETCH_MARKER;
    let mut payload = vec![0; 138 + prefix.len()];
    payload[..prefix.len()].copy_from_slice(prefix);
    payload[5..13].fill(0xff);
    payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    payload[17..21].copy_from_slice(&2u32.to_le_bytes());
    payload[23..29].copy_from_slice(&[0x05, 0x00, 0x01, 0x00, 0x01, 0x00]);
    payload[31..39].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf, 0x00, 0x00, 0x04, 0x00]);
    payload[48..56].copy_from_slice(&1.0f64.to_le_bytes());
    payload[56..58].copy_from_slice(&[0x12, 0x00]);
    for (offset, [u, v]) in [(58, center), (88, start), (104, end)] {
        payload[offset..offset + 8].copy_from_slice(&u.to_le_bytes());
        payload[offset + 8..offset + 16].copy_from_slice(&v.to_le_bytes());
    }
    payload[74..76].copy_from_slice(&11u16.to_le_bytes());
    payload[84..88].copy_from_slice(&1u32.to_le_bytes());
    payload[124..128].copy_from_slice(&7u32.to_le_bytes());
    payload[128..134].copy_from_slice(&[0x00, 0x00, 0x02, 0x00, 0x00, 0x00]);
    payload[134..138].copy_from_slice(&5u32.to_le_bytes());
    payload[138..].copy_from_slice(prefix);
    payload
}

#[test]
fn an_inline_arc_whose_read_points_define_no_minor_arc_keeps_its_handle() {
    let markers = || {
        vec![
            input_marker("arc", 0, SketchInputKind::Arc, Some([0.0, 0.0])),
            input_marker("point", 1, SketchInputKind::Point, Some([0.002, 0.003])),
        ]
    };

    let (resolved, _) = projected_entities(
        markers(),
        inline_arc_record([0.0, 0.0], [0.001, 0.0], [0.0, 0.001]),
    );
    assert!(
        resolved.iter().any(|entity| {
            entity.native_ref.as_deref() == Some("arc")
                && matches!(
                    entity.geometry.definition(),
                    SketchGeometryDefinition::Arc { .. }
                )
        }),
        "the record reaches the inline-arc reader"
    );

    // The end lies 1e-12 mm from the start: distinct when read, one point at
    // the sketch quantum, so center and endpoints define no minor arc.
    let (mut entities, held) = projected_entities(
        markers(),
        inline_arc_record([0.0, 0.0], [0.001, 0.0], [0.001, 1.0e-15]),
    );
    assert!(
        entities
            .iter()
            .all(|entity| entity.native_ref.as_deref() != Some("arc")),
        "the sketch-geometry passes never see the arc handle"
    );
    let [held_arc] = held.handles.as_slice() else {
        panic!("the arc handle keeps its entity");
    };
    assert_eq!(held_arc.native_ref.as_deref(), Some("arc"));
    let id = held_arc.id().clone();

    let mut losses = Vec::new();
    finalize_unread_marker_entities(&[], &mut entities, held, &mut losses);
    let arc = entities
        .iter()
        .position(|entity| entity.native_ref.as_deref() == Some("arc"))
        .expect("finalize adds the held arc handle to its sketch");

    assert_eq!(entities[arc].id(), &id);
    assert_eq!(native_kind(&entities[arc]), Some("sldprt:arc-handle"));
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        SldprtLossCode::SketchHandleUnresolved.kind()
    );
    assert!(losses[0].message.contains(id.as_str()));
}

/// A placeholder whose marker has held `sgSketch` record geometry in its
/// sketch takes that geometry and charges no loss; the same marker in another
/// sketch keeps its role and its loss.
#[test]
fn a_placeholder_with_held_record_geometry_takes_it_without_a_loss() {
    let line = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(1.0, 0.0),
    })
    .unwrap();
    let read = entity("read", placeholder("sldprt:marker-geometry:1"));
    let mut other_sketch = entity("read", placeholder("sldprt:marker-geometry:1"));
    other_sketch.sketch = SketchId::mint("synthetic:test:id#other-sketch").unwrap();
    let mut held = HeldMarkerGeometry::default();
    held.listed.insert(
        (read.sketch.clone(), "marker-read".to_owned()),
        line.clone(),
    );
    let mut entities = vec![read, other_sketch];
    let mut losses = Vec::new();
    finalize_unread_marker_entities(&[], &mut entities, held, &mut losses);
    assert_eq!(entities[0].geometry, line);
    assert_eq!(
        native_kind(&entities[1]),
        Some("sldprt:line-or-circle-handle")
    );
    assert_eq!(losses.len(), 1);
}

/// The 64-byte head of an `sgSketch` record (`docs/formats/sldprt.md`,
/// "Sketch objects").
fn sketch_record_head(out: &mut Vec<u8>) {
    out.extend_from_slice(&[0xff, 0xff, 0x1f, 0x00, 0x03]);
    out.extend_from_slice(&[0xff; 8]);
    out.extend_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0x04, 0x00, 0x02, 0x00]);
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&[0; 9]);
    out.extend_from_slice(&1.0f64.to_le_bytes());
    out.extend_from_slice(&[0; 8]);
}

/// Agreement, in sketch millimetres, of an arc with its point records.
const EPS_TEST_MM: f64 = 1.0e-9;

/// The point record coordinates, in metres, of [`sensed_arc_sketch`].
const SENSED_ARC_POINTS: [[f64; 2]; 3] = [[0.0, 0.0], [0.01, 0.0], [0.0, 0.01]];

/// A lane payload holding one `sgSketch` object: the point records of
/// [`SENSED_ARC_POINTS`] and a counterclockwise arc record from point 1 to
/// point 2 about point 0. Returns the payload, the point record offsets and
/// the arc record offset.
fn sensed_arc_sketch() -> (Vec<u8>, [u64; 3], u64) {
    let offset = |payload: &Vec<u8>| u64::try_from(payload.len()).unwrap();
    let mut payload = vec![0; 7];
    payload.extend_from_slice(&3u32.to_le_bytes());
    payload.extend_from_slice(&[0; 6]);
    payload.extend_from_slice(&1u32.to_le_bytes());
    let mut points = [0; 3];
    for (slot, [u, v]) in points.iter_mut().zip(SENSED_ARC_POINTS) {
        *slot = offset(&payload);
        sketch_record_head(&mut payload);
        payload.extend_from_slice(&0x1eu16.to_le_bytes());
        payload.extend_from_slice(&u.to_le_bytes());
        payload.extend_from_slice(&v.to_le_bytes());
        payload.extend_from_slice(&[0; 10]);
        payload.extend_from_slice(&(-2i32).to_le_bytes());
        payload.extend_from_slice(&[0; 46]);
    }
    payload.extend_from_slice(&0u32.to_le_bytes());
    let arc = offset(&payload);
    sketch_record_head(&mut payload);
    payload.extend_from_slice(&1u16.to_le_bytes());
    payload.extend_from_slice(&2u16.to_le_bytes());
    payload.extend_from_slice(&1u32.to_le_bytes());
    payload.extend_from_slice(&(-1.0f64).to_le_bytes());
    payload.extend_from_slice(&1i32.to_le_bytes());
    payload.extend_from_slice(&0u16.to_le_bytes());
    for _ in 0..4 {
        payload.extend_from_slice(&(-2i32).to_le_bytes());
    }
    payload.extend_from_slice(&[0; 10]);
    (payload, points, arc)
}

/// The entities and losses of [`sensed_arc_sketch`] with point markers `p0`,
/// `p1` and `p2` on its point records and a line-or-circle marker `curve` on
/// its arc record, linked to the two named point markers.
fn sensed_arc_projection(
    endpoints: [&str; 2],
) -> (Vec<SketchEntity>, Vec<cadmpeg_ir::report::loss::LossNote>) {
    let (payload, points, arc) = sensed_arc_sketch();
    let mut markers = Vec::new();
    for (ordinal, ((id, offset), coordinates)) in ["p0", "p1", "p2"]
        .into_iter()
        .zip(points)
        .zip(SENSED_ARC_POINTS)
        .enumerate()
    {
        markers.push(marker_at(
            id,
            u32::try_from(ordinal).unwrap(),
            offset,
            SketchInputKind::Point,
            Some(coordinates),
        ));
    }
    let mut curve = marker_at("curve", 3, arc, SketchInputKind::LineOrCircle, None);
    curve.links = SketchInputLinks::new(
        4,
        endpoints
            .into_iter()
            .map(|entity_ref| SketchInputLink {
                local_id: 0,
                entity_ref: entity_ref.into(),
            })
            .collect(),
    );
    markers.push(curve);
    let (mut entities, held) = projected_entities(markers, payload);
    let mut losses = Vec::new();
    finalize_unread_marker_entities(&[], &mut entities, held, &mut losses);
    (entities, losses)
}

fn marker_entity<'a>(entities: &'a [SketchEntity], marker: &str) -> &'a SketchEntity {
    entities
        .iter()
        .find(|entity| entity.native_ref.as_deref() == Some(marker))
        .unwrap_or_else(|| panic!("marker {marker} is projected"))
}

/// A curve marker whose `sgSketch` record is an arc takes that arc, between
/// the point records at the record's positions, whatever endpoints its marker
/// links name.
#[test]
fn a_curve_marker_takes_the_arc_of_its_record() {
    for links in [["p1", "p2"], ["p0", "p1"]] {
        a_curve_marker_takes_the_arc_of_its_record_with_links(links);
    }
}

fn a_curve_marker_takes_the_arc_of_its_record_with_links(links: [&str; 2]) {
    let (entities, losses) = sensed_arc_projection(links);
    let position = |marker: &str| match marker_entity(&entities, marker).geometry.definition() {
        SketchGeometryDefinition::Point { position } => position.get(),
        other => panic!("marker {marker} is not a point: {other:?}"),
    };
    let (center, end) = (position("p0"), position("p2"));
    let SketchGeometryDefinition::Arc {
        center: arc_center,
        radius,
        ..
    } = marker_entity(&entities, "curve").geometry.definition()
    else {
        panic!("the sensed record replaces the line with its arc");
    };
    let arc_center = arc_center.get();
    assert!((arc_center.u - center.u).hypot(arc_center.v - center.v) < EPS_TEST_MM);
    assert!((radius.get() - (end.u - center.u).hypot(end.v - center.v)).abs() < EPS_TEST_MM);
    assert_eq!(
        marker_entity(&entities, "curve").endpoint_refs,
        ["p1", "p2"]
    );
    assert!(losses.is_empty(), "{losses:?}");
}

/// A lane payload holding one `sgSketch` object with point records at
/// `points` (metres) and curve records `(start, end, selector)` that are
/// lines. Returns the payload and the record offsets.
fn framed_line_sketch(
    points: &[[f64; 2]],
    lines: &[(u16, u16, u16)],
) -> (Vec<u8>, Vec<u64>, Vec<u64>) {
    let offset = |payload: &Vec<u8>| u64::try_from(payload.len()).unwrap();
    let mut payload = vec![0; 7];
    payload.extend_from_slice(&u32::try_from(points.len()).unwrap().to_le_bytes());
    payload.extend_from_slice(&[0; 6]);
    payload.extend_from_slice(&1u32.to_le_bytes());
    let mut point_offsets = Vec::new();
    for [u, v] in points {
        point_offsets.push(offset(&payload));
        sketch_record_head(&mut payload);
        payload.extend_from_slice(&0x1eu16.to_le_bytes());
        payload.extend_from_slice(&u.to_le_bytes());
        payload.extend_from_slice(&v.to_le_bytes());
        payload.extend_from_slice(&[0; 10]);
        payload.extend_from_slice(&(-2i32).to_le_bytes());
        payload.extend_from_slice(&[0; 46]);
    }
    payload.extend_from_slice(&0u32.to_le_bytes());
    let mut line_offsets = Vec::new();
    for &(start, end, selector) in lines {
        let at = payload.len();
        line_offsets.push(offset(&payload));
        sketch_record_head(&mut payload);
        payload[at + 37..at + 39].copy_from_slice(&selector.to_le_bytes());
        payload.extend_from_slice(&start.to_le_bytes());
        payload.extend_from_slice(&end.to_le_bytes());
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&(-1.0f64).to_le_bytes());
        payload.extend_from_slice(&0i32.to_le_bytes());
        payload.extend_from_slice(&[0; 12]);
    }
    (payload, point_offsets, line_offsets)
}

/// A 460 x 60 mm box whose point records are listed corner by corner and
/// whose curve records join them edge by edge, with a construction diagonal
/// (selector `12`). Two point markers carry the curve code `1` at +17, one
/// curve marker the point code `0`: the record, not the marker code, decides.
/// No marker stands on the last edge's record; it is read all the same. The
/// four edges form one closed chain through the corner markers, and the
/// diagonal is construction.
#[test]
fn a_framed_box_projects_the_rectangle_of_its_records() {
    let corners = [[0.0, 0.0], [0.46, 0.06], [0.0, 0.06], [0.46, 0.0]];
    let (payload, points, lines) = framed_line_sketch(
        &corners,
        &[(0, 3, 4), (0, 2, 4), (2, 1, 4), (3, 1, 4), (0, 1, 12)],
    );
    let mut markers = Vec::new();
    for (index, (offset, coordinates)) in points.iter().zip(corners).enumerate() {
        let kind = if index < 2 {
            SketchInputKind::Point
        } else {
            SketchInputKind::LineOrCircle
        };
        markers.push(marker_at(
            &format!("corner{index}"),
            u32::try_from(index).unwrap(),
            *offset,
            kind,
            Some(coordinates),
        ));
    }
    for (index, offset) in lines.iter().enumerate() {
        if index == 3 {
            continue;
        }
        let kind = if index == 0 {
            SketchInputKind::Point
        } else {
            SketchInputKind::LineOrCircle
        };
        markers.push(marker_at(
            &format!("edge{index}"),
            u32::try_from(4 + index).unwrap(),
            *offset,
            kind,
            None,
        ));
    }
    let (entities, _) = projected_entities(markers, payload);
    for index in 0..4 {
        assert!(matches!(
            marker_entity(&entities, &format!("corner{index}"))
                .geometry
                .definition(),
            SketchGeometryDefinition::Point { .. }
        ));
    }
    let mut incidence = BTreeMap::<String, usize>::new();
    for index in 0..4 {
        let edge = if index == 3 {
            entities
                .iter()
                .find(|entity| {
                    entity
                        .id()
                        .as_str()
                        .ends_with(&format!(":record:{}", lines[3]))
                })
                .expect("the unmarked edge record is read")
        } else {
            marker_entity(&entities, &format!("edge{index}"))
        };
        let SketchGeometryDefinition::Line { start, end } = *edge.geometry.definition() else {
            panic!("edge {index} is a line: {:?}", edge.geometry);
        };
        let (start, end) = (start.get(), end.get());
        assert!(
            (start.u - end.u).abs() < EPS_TEST_MM || (start.v - end.v).abs() < EPS_TEST_MM,
            "edge {index} runs along an axis: {start:?} {end:?}"
        );
        assert!(!edge.construction);
        for endpoint in &edge.endpoint_refs {
            *incidence.entry(endpoint.clone()).or_default() += 1;
        }
    }
    assert_eq!(
        incidence,
        (0..4)
            .map(|index| (format!("corner{index}"), 2))
            .collect::<BTreeMap<_, _>>()
    );
    assert!(marker_entity(&entities, "edge4").construction);
}

/// A line that a later reading gives a marker whose curve record states
/// another form charges one loss naming the entity and the form; a line whose
/// marker has no such record, or that an earlier finalize pass charged,
/// charges none.
#[test]
fn a_later_line_on_a_marker_whose_record_is_not_a_line_charges_the_conflict() {
    let line = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(1.0, 0.0),
    })
    .unwrap();
    let earlier = vec![entity("earlier", line.clone())];
    let mut entities = earlier.clone();
    entities.extend([entity("relation", line.clone()), entity("plain", line)]);
    let mut held = HeldMarkerGeometry::after_finalized(&earlier);
    for marker in ["marker-earlier", "marker-relation"] {
        held.unchecked
            .insert(marker.to_owned(), ChordConflict::Unchecked);
    }
    let mut losses = Vec::new();
    finalize_unread_marker_entities(&[], &mut entities, held, &mut losses);
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}");
    };
    assert_eq!(loss.code, SldprtLossCode::SketchArcRecordConflict.kind());
    assert!(loss.message.contains(entities[1].id().as_str()));
    assert!(loss.message.contains("does not compare the record"));
}

#[test]
fn each_remaining_sketch_on_the_retained_front_fallback_charges_one_assumed_plane() {
    let sketch = SketchId::mint("sldprt:model:sketch#markers:lane:4").unwrap();
    let replaced = SketchId::mint("sldprt:model:sketch#compact:lane:4").unwrap();
    let held = HeldMarkerGeometry {
        assumed_planes: vec![replaced, sketch.clone(), sketch.clone()],
        ..HeldMarkerGeometry::default()
    };
    let sketches = [cadmpeg_ir::sketches::Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    }];
    let mut entities = Vec::new();
    let mut losses = Vec::new();
    finalize_unread_marker_entities(&sketches, &mut entities, held, &mut losses);
    assert_eq!(
        losses
            .iter()
            .map(|loss| loss.code.clone())
            .collect::<Vec<_>>(),
        vec![SldprtLossCode::SketchPlaneAssumed.kind()]
    );
    assert!(losses[0].message.contains(sketch.as_str()));
}
