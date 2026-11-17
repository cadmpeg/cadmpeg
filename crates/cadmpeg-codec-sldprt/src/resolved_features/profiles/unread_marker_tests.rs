// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};

use super::{finalize_unread_marker_entities, project_marker_backed_sketches};
use crate::loss::SldprtLossCode;
use crate::records::{
    Feature as NativeFeature, FeatureHistory, FeatureInputLane, FeatureSource, SketchInputEntity,
    SketchInputKind,
};

fn input_marker(
    id: &str,
    ordinal: u32,
    kind: SketchInputKind,
    coordinates_m: Option<[f64; 2]>,
) -> SketchInputEntity {
    let mut marker = SketchInputEntity::new(id, "lane", ordinal, 0, kind);
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

/// The entities and held arc handles `project_marker_backed_sketches` emits
/// for `markers` of one sketch feature whose lane holds `payload`.
fn projected_entities(
    markers: Vec<SketchInputEntity>,
    payload: Vec<u8>,
) -> (Vec<SketchEntity>, Vec<SketchEntity>) {
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
    let mut held = Vec::new();
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
    finalize_unread_marker_entities(&mut entities, Vec::new(), &mut losses);

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
    let [held_arc] = held.as_slice() else {
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
