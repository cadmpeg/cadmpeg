// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};

use super::{
    finalize_unread_marker_entities, project_marker_backed_sketches, ChordConflict,
    HeldMarkerGeometry, ListedEntity, MarkerSketchEntities,
};
use crate::loss::SldprtLossCode;
use crate::records::{
    Feature as NativeFeature, FeatureHistory, FeatureInputLane, FeatureSource, SketchInputEntity,
    SketchInputKind, SketchInputLink, SketchInputLinks, SketchRelationKind,
};
use crate::test_support::sketch_records::{
    framed_line_sketch, framed_line_sketch_with_states, sketch_record_head,
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
    let (_, entities, held) = projection(markers, payload);
    (entities, held)
}

/// The sketches, entities and held marker geometry
/// `project_marker_backed_sketches` emits for `markers` of one sketch feature
/// whose lane holds `payload`.
fn projection(
    markers: Vec<SketchInputEntity>,
    payload: Vec<u8>,
) -> (Vec<Sketch>, Vec<SketchEntity>, HeldMarkerGeometry) {
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
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
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
        MarkerSketchEntities {
            projected: &mut entities,
            held: &mut held,
        },
        crate::resolved_features::profiles::MarkerSources {
            histories: &histories,
            lanes: &lanes,
            solid: &crate::resolved_features::stream_readings::SolidEdges::default(),
        },
        &mut Vec::new(),
    )
    .expect("marker profile projection fits service policy");
    (sketches, entities, held)
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
    finalize_unread_marker_entities(&mut entities, held, &mut losses);

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
    finalize_unread_marker_entities(&mut entities, HeldMarkerGeometry::default(), &mut losses);

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
pub(super) fn inline_arc_record(center: [f64; 2], start: [f64; 2], end: [f64; 2]) -> Vec<u8> {
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
    finalize_unread_marker_entities(&mut entities, held, &mut losses);
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

/// A placeholder that takes held `sgSketch` record geometry charges the
/// losses held with it.
#[test]
fn a_placeholder_with_held_record_geometry_charges_the_losses_held_with_it() {
    let arc = SketchGeometry::try_from(SketchGeometryDefinition::Arc {
        center: Point2::new(0.0, 0.0),
        radius: cadmpeg_ir::scalar::Length::new(1.0).unwrap(),
        start_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
        end_angle: cadmpeg_ir::scalar::Angle::new(1.0).unwrap(),
    })
    .unwrap();
    let mut entities = vec![entity("off-chord", placeholder("sldprt:marker-geometry:1"))];
    let mut held = HeldMarkerGeometry::default();
    held.listed.insert(
        (entities[0].sketch.clone(), "marker-off-chord".to_owned()),
        (
            ListedEntity {
                geometry: arc.clone(),
                endpoints: None,
                construction: false,
                outside_profile: false,
                index_basis_loss: None,
            },
            vec![SldprtLossCode::SketchArcRecordOffChord.note("off chord")],
        ),
    );
    let mut losses = Vec::new();
    finalize_unread_marker_entities(&mut entities, held, &mut losses);
    assert_eq!(entities[0].geometry, arc);
    assert_eq!(
        losses
            .iter()
            .map(|loss| loss.code.clone())
            .collect::<Vec<_>>(),
        vec![SldprtLossCode::SketchArcRecordOffChord.kind()]
    );
}

/// A placeholder whose marker has held `sgSketch` record geometry in its
/// sketch takes that geometry and the record's construction mark and charges
/// no loss; the same marker in another sketch keeps its role and its loss.
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
        (
            ListedEntity {
                geometry: line.clone(),
                endpoints: None,
                construction: true,
                outside_profile: false,
                index_basis_loss: None,
            },
            Vec::new(),
        ),
    );
    let mut entities = vec![read, other_sketch];
    let mut losses = Vec::new();
    finalize_unread_marker_entities(&mut entities, held, &mut losses);
    assert_eq!(entities[0].geometry, line);
    assert!(entities[0].construction);
    assert_eq!(
        native_kind(&entities[1]),
        Some("sldprt:line-or-circle-handle")
    );
    assert_eq!(losses.len(), 1);
}

/// A placeholder that takes the held geometry of a curve record of state `0`
/// is handed on as a curve outside its sketch's profile.
#[test]
fn a_placeholder_with_held_state_zero_record_geometry_is_outside_the_profile() {
    let line = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(1.0, 0.0),
    })
    .unwrap();
    let read = entity("read", placeholder("sldprt:marker-geometry:1"));
    let id = read.id().clone();
    let mut held = HeldMarkerGeometry::default();
    held.listed.insert(
        (read.sketch.clone(), "marker-read".to_owned()),
        (
            ListedEntity {
                geometry: line,
                endpoints: None,
                construction: false,
                outside_profile: true,
                index_basis_loss: None,
            },
            Vec::new(),
        ),
    );
    let mut entities = vec![read];
    let finalized = finalize_unread_marker_entities(&mut entities, held, &mut Vec::new());
    assert_eq!(
        finalized.outside_profile,
        std::collections::HashSet::from([id])
    );
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
    let state = usize::try_from(arc).unwrap() + 29;
    payload[state..state + 2].copy_from_slice(&1u16.to_le_bytes());
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
    selector: u16,
    sense: i32,
) -> (Vec<SketchEntity>, Vec<cadmpeg_ir::report::loss::LossNote>) {
    let (mut payload, points, arc) = sensed_arc_sketch();
    let arc_offset = usize::try_from(arc).unwrap();
    let selector_at = arc_offset + crate::layout::sketch_record_head::SELECTOR;
    payload[selector_at..selector_at + 2].copy_from_slice(&selector.to_le_bytes());
    let sense_at = arc_offset + crate::layout::sketch_arc_record::SENSE;
    payload[sense_at..sense_at + 4].copy_from_slice(&sense.to_le_bytes());
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
    finalize_unread_marker_entities(&mut entities, held, &mut losses);
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
    let (entities, losses) = sensed_arc_projection(links, 68, 1);
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
    finalize_unread_marker_entities(&mut entities, held, &mut losses);
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}");
    };
    assert_eq!(loss.code, SldprtLossCode::SketchArcRecordConflict.kind());
    assert!(loss.message.contains(entities[1].id().as_str()));
    assert!(loss.message.contains("does not compare the record"));
}

/// The corners, in metres, of a 10 mm square.
const SQUARE: [[f64; 2]; 4] = [[0.0, 0.0], [0.01, 0.0], [0.01, 0.01], [0.0, 0.01]];

/// Point markers `corner0` to `corner3` on the point records at `points`.
fn corner_markers(points: &[u64]) -> Vec<SketchInputEntity> {
    points
        .iter()
        .zip(SQUARE)
        .enumerate()
        .map(|(index, (offset, coordinates))| {
            marker_at(
                &format!("corner{index}"),
                u32::try_from(index).unwrap(),
                *offset,
                SketchInputKind::Point,
                Some(coordinates),
            )
        })
        .collect()
}

/// The entity read from the unmarked curve record at `offset`.
fn record_entity(entities: &[SketchEntity], offset: u64) -> &SketchEntity {
    entities
        .iter()
        .find(|entity| entity.id().as_str().ends_with(&format!(":record:{offset}")))
        .unwrap_or_else(|| panic!("the record at {offset} is read"))
}

/// A square of profile lines (state `1`) and a diagonal of state `0` with
/// the profile selector `4`: the diagonal is read as a line of the sketch,
/// not construction, and the profile is the square alone.
#[test]
fn a_line_of_state_zero_is_read_and_joins_no_profile() {
    let (payload, points, lines) = framed_line_sketch_with_states(
        &SQUARE,
        &[
            (0, 1, 4, 1),
            (1, 2, 4, 1),
            (2, 3, 4, 1),
            (3, 0, 4, 1),
            (0, 2, 4, 0),
        ],
    );
    let (sketches, entities, _) = projection(corner_markers(&points), payload);
    let [sketch] = sketches.as_slice() else {
        panic!("one sketch: {sketches:?}");
    };
    let diagonal = record_entity(&entities, lines[4]);
    assert!(matches!(
        diagonal.geometry.definition(),
        SketchGeometryDefinition::Line { .. }
    ));
    assert!(!diagonal.construction);
    let profiles = sketch.profiles.iter().collect::<Vec<_>>();
    let [profile] = profiles.as_slice() else {
        panic!("one profile: {profiles:?}");
    };
    let mut members = profile
        .iter()
        .map(|entity_use| entity_use.entity.clone())
        .collect::<Vec<_>>();
    members.sort();
    let mut sides = lines[..4]
        .iter()
        .map(|&offset| record_entity(&entities, offset).id().clone())
        .collect::<Vec<_>>();
    sides.sort();
    assert_eq!(members, sides);
}

/// A marker that the marker layouts skip, here one whose u32 at +17 holds a
/// relation code, is admitted when its record is a curve record that reads,
/// and takes the record's line.
#[test]
fn a_skipped_marker_whose_curve_record_reads_takes_the_record_line() {
    let (payload, points, lines) =
        framed_line_sketch(&SQUARE, &[(0, 1, 4), (1, 2, 4), (2, 3, 4), (3, 0, 4)]);
    let mut markers = corner_markers(&points);
    markers.push(marker_at(
        "side",
        4,
        lines[1],
        SketchInputKind::Relation(SketchRelationKind::Vertical),
        None,
    ));
    let (entities, _) = projected_entities(markers, payload);
    let side = marker_entity(&entities, "side");
    let SketchGeometryDefinition::Line { start, end } = *side.geometry.definition() else {
        panic!("the side is a line: {:?}", side.geometry);
    };
    let (start, end) = (start.get(), end.get());
    assert!((start.u - end.u).abs() < EPS_TEST_MM);
    assert!(((start.v - end.v).abs() - 10.0).abs() < EPS_TEST_MM);
    assert_eq!(side.endpoint_refs, ["corner1", "corner2"]);
}

/// A marker that the marker layouts skip, admitted because its record reads,
/// whose record then gives no entity in the sketch, here a point record
/// beyond the integer grid of the sketch's marker transform, gives no entity
/// and charges one loss naming the sketch and the marker.
#[test]
fn a_skipped_marker_whose_record_then_gives_no_entity_is_named() {
    let mut corners = SQUARE.to_vec();
    corners.push([1.0e9, 0.0]);
    let (payload, points, _) =
        framed_line_sketch(&corners, &[(0, 1, 4), (1, 2, 4), (2, 3, 4), (3, 0, 4)]);
    let mut markers = corner_markers(&points);
    markers.push(marker_at(
        "far",
        4,
        points[4],
        SketchInputKind::Relation(SketchRelationKind::Vertical),
        None,
    ));
    let (sketches, mut entities, held) = projection(markers, payload);
    let [sketch] = sketches.as_slice() else {
        panic!("one sketch: {sketches:?}");
    };
    assert!(entities
        .iter()
        .all(|entity| entity.native_ref.as_deref() != Some("far")));
    let mut losses = Vec::new();
    finalize_unread_marker_entities(&mut entities, held, &mut losses);
    let unread = losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::SketchMarkerRecordUnread.kind())
        .collect::<Vec<_>>();
    let [loss] = unread.as_slice() else {
        panic!("one unread marker: {losses:?}");
    };
    assert!(loss.message.contains(sketch.id.as_str()));
    assert!(loss.message.contains("marker far,"));
    assert!(loss.message.contains("no point or curve"));
}

#[test]
fn geometry_locus_arc_projection_reports_its_point_record_index_basis() {
    for end in [1u16, 2] {
        let (mut payload, points, arc) = sensed_arc_sketch();
        let offset = usize::try_from(arc).unwrap();
        payload[offset + 23..offset + 27].copy_from_slice(&[5, 0, 1, 0]);
        payload[offset + 66..offset + 68].copy_from_slice(&end.to_le_bytes());
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
        markers.push(marker_at(
            "curve",
            3,
            arc,
            SketchInputKind::LineOrCircle,
            None,
        ));
        let (mut entities, held) = projected_entities(markers, payload);
        let mut losses = Vec::new();
        finalize_unread_marker_entities(&mut entities, held, &mut losses);
        let geometry = marker_entity(&entities, "curve").geometry.definition();
        assert!(matches!(
            (end, geometry),
            (1, SketchGeometryDefinition::Circle { .. })
                | (2, SketchGeometryDefinition::Arc { .. })
        ));
        assert_eq!(
            losses
                .iter()
                .filter(
                    |loss| loss.code == SldprtLossCode::SketchPointRecordIndexBasisAssumed.kind()
                )
                .count(),
            1
        );
    }
}

#[test]
fn a_roster_curve_precedes_marker_links_and_keeps_its_clockwise_sense() {
    let (entities, losses) = sensed_arc_projection(["p0", "p1"], 12, -1);
    let curve = marker_entity(&entities, "curve");
    let SketchGeometryDefinition::Arc {
        radius,
        start_angle,
        end_angle,
        ..
    } = curve.geometry.definition()
    else {
        panic!("the roster record gives its arc before the linked chord");
    };
    assert!(curve.construction);
    assert!((radius.get() - 10.0).abs() < EPS_TEST_MM);
    assert!(
        (end_angle.get() - start_angle.get() - 3.0 * std::f64::consts::FRAC_PI_2).abs()
            < super::super::SKETCH_ANGLE_TOLERANCE
    );
    assert!(losses.is_empty(), "{losses:?}");
}
