// SPDX-License-Identifier: Apache-2.0

//! The curve records of an `sgSketch` list that the list reading takes no
//! geometry from (`docs/formats/sldprt.md`, "Sketch objects").

use std::collections::BTreeMap;

use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::sketches::{SketchEntity, SketchGeometry, SketchGeometryDefinition, SketchId};

use super::{
    marker_is_construction, project_marker_backed_sketches, HeldMarkerGeometry,
    MarkerSketchEntities, MarkerSources, SketchRecordLists,
};
use crate::loss::SldprtLossCode;
use crate::records::{
    Feature as NativeFeature, FeatureHistory, FeatureInputLane, FeatureSource, SketchInputKind,
};
use crate::resolved_features::markers::{
    report_listed_curve_relation_drops, sketch_input_entities,
};

const F32_MINUS_ONE: [u8; 4] = [0x00, 0x00, 0x80, 0xbf];
const F64_MINUS_ONE: [u8; 8] = [0, 0, 0, 0, 0, 0, 0xf0, 0xbf];
/// The +17 value of a horizontal relation code.
const CODE_FOUR: u32 = 4;

fn head(out: &mut Vec<u8>, code: u32, selector: u16) {
    out.extend_from_slice(&[0xff, 0xff, 0x1f, 0x00, 0x03]);
    out.extend_from_slice(&[0xff; 8]);
    out.extend_from_slice(&F32_MINUS_ONE);
    out.extend_from_slice(&code.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&[0x04, 0x00, 0x02, 0x00]);
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&F32_MINUS_ONE);
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&selector.to_le_bytes());
    out.extend_from_slice(&[0; 9]);
    out.extend_from_slice(&1.0f64.to_le_bytes());
    out.extend_from_slice(&[0; 8]);
}

fn point(out: &mut Vec<u8>, u: f64, v: f64) -> usize {
    let at = out.len();
    head(out, 0, 4);
    out.extend_from_slice(&0x1eu16.to_le_bytes());
    out.extend_from_slice(&u.to_le_bytes());
    out.extend_from_slice(&v.to_le_bytes());
    out.extend_from_slice(&[0; 10]);
    out.extend_from_slice(&0xffff_fffeu32.to_le_bytes());
    out.extend_from_slice(&[0; 46]);
    at
}

/// A 92-byte line record with the given +17 code, selector, endpoint indices,
/// trailer u32 at +84 and identity at +88.
fn line(
    out: &mut Vec<u8>,
    code: u32,
    selector: u16,
    indices: [u16; 2],
    trailer: u32,
    identity: u32,
) -> usize {
    let at = out.len();
    head(out, code, selector);
    out.extend_from_slice(&indices[0].to_le_bytes());
    out.extend_from_slice(&indices[1].to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&F64_MINUS_ONE);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&[0; 2]);
    out.extend_from_slice(&trailer.to_le_bytes());
    out.extend_from_slice(&identity.to_le_bytes());
    at
}

/// A payload of a sketch with two point records and one line record.
fn sketch_with_line(
    code: u32,
    selector: u16,
    indices: [u16; 2],
    trailer: u32,
    identity: u32,
) -> (Vec<u8>, usize) {
    let mut payload = vec![0; 7];
    payload.extend_from_slice(&2u32.to_le_bytes());
    payload.extend_from_slice(&[0; 6]);
    payload.extend_from_slice(&1u32.to_le_bytes());
    point(&mut payload, 0.0, 0.0);
    point(&mut payload, 0.01, 0.0);
    payload.extend_from_slice(&0u32.to_le_bytes());
    let offset = line(&mut payload, code, selector, indices, trailer, identity);
    (payload, offset)
}

fn sketch_with_arc(code: u32, indices: [u16; 2]) -> (Vec<u8>, usize) {
    let mut payload = vec![0; 7];
    payload.extend_from_slice(&3u32.to_le_bytes());
    payload.extend_from_slice(&[0; 6]);
    payload.extend_from_slice(&1u32.to_le_bytes());
    point(&mut payload, 0.01, 0.0);
    point(&mut payload, 0.0, 0.01);
    point(&mut payload, 0.0, 0.0);
    payload.extend_from_slice(&0u32.to_le_bytes());
    let offset = payload.len();
    head(&mut payload, code, 4);
    payload.extend_from_slice(&indices[0].to_le_bytes());
    payload.extend_from_slice(&indices[1].to_le_bytes());
    payload.extend_from_slice(&1u32.to_le_bytes());
    payload.extend_from_slice(&F64_MINUS_ONE);
    payload.extend_from_slice(&1i32.to_le_bytes());
    payload.extend_from_slice(&2u16.to_le_bytes());
    for _ in 0..4 {
        payload.extend_from_slice(&(-2i32).to_le_bytes());
    }
    payload.extend_from_slice(&0u16.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&u32::MAX.to_le_bytes());
    (payload, offset)
}

/// The entity `project_marker_backed_sketches` emits for the curve record at
/// `curve`, its input kind, and the lane's and the projection's losses.
fn projected_curve(
    payload: Vec<u8>,
    curve: usize,
) -> (SketchInputKind, Option<SketchEntity>, Vec<LossNote>) {
    projected_curve_in(payload, curve, None)
}

/// [`projected_curve`] for a sketch feature bound to `sketch` before the
/// marker projection runs.
fn projected_curve_in(
    payload: Vec<u8>,
    curve: usize,
    sketch: Option<SketchId>,
) -> (SketchInputKind, Option<SketchEntity>, Vec<LossNote>) {
    let (kind, marker_id, entities, held, losses) = projection(payload, curve, sketch);
    let entity = entities
        .into_iter()
        .chain(held.handles)
        .find(|entity| entity.native_ref.as_deref() == Some(marker_id.as_str()));
    (kind, entity, losses)
}

/// What `project_marker_backed_sketches` emits for a sketch whose curve
/// record at `curve` is a marker, its feature bound to `sketch` before the
/// projection runs: the marker's input kind and reference, the projected
/// entities, the held geometry, and the projection losses.
fn projection(
    payload: Vec<u8>,
    curve: usize,
    sketch: Option<SketchId>,
) -> (
    SketchInputKind,
    String,
    Vec<SketchEntity>,
    HeldMarkerGeometry,
    Vec<LossNote>,
) {
    let mut markers = sketch_input_entities(&payload, "lane");
    for marker in &mut markers {
        marker.feature_ref = Some("feature-native".into());
    }
    let marker = markers
        .iter()
        .find(|marker| usize::try_from(marker.offset()) == Ok(curve))
        .expect("the curve record is a sketch marker");
    let kind = marker.kind();
    let marker_id = marker.id().to_owned();
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
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(sketch),
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
    let wire = serde_json::to_value(&lanes).expect("lane serializes");
    let restored: Vec<FeatureInputLane> = serde_json::from_value(wire).expect("lane re-admits");
    assert_eq!(restored, lanes);
    let mut sketches = Vec::new();
    let mut entities = Vec::new();
    let mut held = HeldMarkerGeometry::default();
    let mut losses = Vec::new();
    report_listed_curve_relation_drops(&lanes, &histories, &mut losses);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("fixture fits service policy");
    project_marker_backed_sketches(
        &ctx,
        &mut features,
        &mut sketches,
        MarkerSketchEntities {
            projected: &mut entities,
            held: &mut held,
        },
        MarkerSources {
            histories: &histories,
            lanes: &lanes,
            solid: &crate::resolved_features::stream_readings::SolidEdges::default(),
        },
        &mut losses,
    )
    .expect("projection fits service policy");
    (kind, marker_id, entities, held, losses)
}

/// `docs/formats/sldprt.md` §2: a curve record of state `0` is a curve
/// outside the sketch's profile, and the projection hands its entity on as
/// one; a curve record of state `1` is a curve of the profile.
#[test]
fn a_curve_record_of_state_zero_is_handed_on_as_outside_the_profile() {
    for (state, outside) in [(0u16, true), (1, false)] {
        let (mut payload, curve) = sketch_with_line(0, 4, [0, 1], 0, 7);
        let at = curve + crate::layout::sketch_record_head::STATE;
        payload[at..at + 2].copy_from_slice(&state.to_le_bytes());
        let (_, marker_id, entities, held, _) = projection(payload, curve, None);
        let line = entities
            .iter()
            .find(|entity| entity.native_ref.as_deref() == Some(marker_id.as_str()))
            .expect("the line record is projected");
        assert!(matches!(
            line.geometry.definition(),
            SketchGeometryDefinition::Line { .. }
        ));
        assert_eq!(
            held.outside_profile.contains(line.id()),
            outside,
            "state {state}"
        );
    }
}

/// `docs/formats/sldprt.md` §2.3: the curve type selects the curve; the +17
/// value is no relation code. A line that the list reading takes no geometry
/// from stays a line handle, not a relation.
fn assert_unresolved_line_handle(payload: Vec<u8>, curve: usize) {
    let (kind, entity, losses) = projected_curve(payload, curve);
    assert_eq!(kind, SketchInputKind::LineOrCircle);
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.code == SldprtLossCode::SketchListedCurveRelationDropped.kind())
            .count(),
        1
    );
    let entity = entity.expect("the curve record keeps an entity");
    assert!(matches!(
        entity.geometry.definition(),
        SketchGeometryDefinition::Native { native_kind }
            if native_kind.as_str().starts_with("sldprt:marker-geometry:")
    ));
}

/// `docs/formats/sldprt.md` §2.3: a line with an identifier trailer indexes
/// the point records of its sketch by position like every other curve record,
/// so it is the line between them and its +17 value is no relation code.
#[test]
fn an_identifier_trailer_line_with_a_relation_code_is_a_line() {
    for (selector, trailer) in [(4, 0), (12, 1)] {
        let (payload, curve) = sketch_with_line(CODE_FOUR, selector, [0, 1], trailer, 7);
        let (kind, entity, losses) = projected_curve(payload, curve);
        assert_eq!(kind, SketchInputKind::LineOrCircle);
        assert!(matches!(
            entity.expect("the line is projected").geometry.definition(),
            SketchGeometryDefinition::Line { .. }
        ));
        assert_eq!(
            losses
                .iter()
                .filter(|loss| loss.code == SldprtLossCode::SketchListedCurveRelationDropped.kind())
                .count(),
            1
        );
    }
}

#[test]
fn a_line_with_equal_endpoint_indices_and_a_relation_code_is_a_line_handle() {
    let (payload, curve) = sketch_with_line(CODE_FOUR, 4, [1, 1], 0, u32::MAX);
    assert_unresolved_line_handle(payload, curve);
}

#[test]
fn a_readable_line_with_a_relation_code_is_a_line() {
    for code in [CODE_FOUR, 5, 7, 86] {
        let (payload, curve) = sketch_with_line(code, 4, [0, 1], 0, u32::MAX);
        let (kind, entity, losses) = projected_curve(payload, curve);
        assert_eq!(kind, SketchInputKind::LineOrCircle);
        let entity = entity.expect("the line is projected");
        assert!(matches!(
            entity.geometry.definition(),
            SketchGeometryDefinition::Line { .. }
        ));
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            SldprtLossCode::SketchListedCurveRelationDropped.kind()
        );
        assert!(losses[0]
            .message
            .contains(&format!("relation code {code} at +17")));
        assert!(losses[0].message.contains("listed line"));
        assert!(losses[0].message.contains("generated-profile"));
        assert!(losses[0]
            .message
            .contains(entity.native_ref.as_deref().expect("marker reference")));
    }
}

/// DI-77: the dropped relation code is named once, also when the sketch's
/// curves come from its profile stream and no marker of it is projected.
#[test]
fn a_profile_stream_sketch_names_its_dropped_relation_code() {
    let (payload, curve) = sketch_with_line(CODE_FOUR, 4, [0, 1], 0, u32::MAX);
    let sketch = SketchId::mint("sldprt:model:sketch#profile:lane:1").expect("identity grammar");
    let (kind, entity, losses) = projected_curve_in(payload, curve, Some(sketch));
    assert_eq!(kind, SketchInputKind::LineOrCircle);
    assert!(entity.is_none());
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        SldprtLossCode::SketchListedCurveRelationDropped.kind()
    );
    assert!(losses[0].message.contains("relation code 4 at +17"));
    assert!(losses[0].message.contains("generated-profile"));
}

#[test]
fn a_readable_line_with_another_code_has_no_relation_drop_loss() {
    for code in [0, 1] {
        let (payload, curve) = sketch_with_line(code, 4, [0, 1], 0, u32::MAX);
        let (kind, entity, losses) = projected_curve(payload, curve);
        assert_eq!(kind, SketchInputKind::LineOrCircle);
        assert!(matches!(
            entity.expect("the line is projected").geometry.definition(),
            SketchGeometryDefinition::Line { .. }
        ));
        assert!(losses.is_empty());
    }
}

#[test]
fn a_framed_arc_reports_its_dropped_parallel_relation() {
    let (payload, curve) = sketch_with_arc(7, [0, 1]);
    let (kind, entity, losses) = projected_curve(payload, curve);
    assert_eq!(kind, SketchInputKind::Arc);
    assert!(matches!(
        entity.expect("the arc is projected").geometry.definition(),
        SketchGeometryDefinition::Arc { .. }
    ));
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        SldprtLossCode::SketchListedCurveRelationDropped.kind()
    );
    assert!(losses[0].message.contains("listed arc"));
    assert!(losses[0].message.contains("relation code 7 at +17"));
}

#[test]
fn a_framed_circle_reports_its_dropped_horizontal_relation() {
    let (payload, curve) = sketch_with_arc(CODE_FOUR, [0, 0]);
    let (kind, entity, losses) = projected_curve(payload, curve);
    assert_eq!(kind, SketchInputKind::LineOrCircle);
    assert!(matches!(
        entity
            .expect("the circle is projected")
            .geometry
            .definition(),
        SketchGeometryDefinition::Circle { .. }
    ));
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        SldprtLossCode::SketchListedCurveRelationDropped.kind()
    );
    assert!(losses[0].message.contains("listed circle"));
    assert!(losses[0].message.contains("relation code 4 at +17"));
}

#[test]
fn a_curve_record_with_another_selector_keeps_its_relation_kind() {
    let (payload, curve) = sketch_with_line(CODE_FOUR, 68, [0, 1], 0, u32::MAX);
    let (kind, _, losses) = projected_curve(payload, curve);
    assert!(matches!(kind, SketchInputKind::Relation(_)));
    assert!(losses
        .iter()
        .all(|loss| loss.code != SldprtLossCode::SketchListedCurveRelationDropped.kind()));
}

#[test]
fn selector_twelve_role_one_circle_construction_agrees_between_projection_paths() {
    let (mut payload, curve) = sketch_with_line(1, 12, [1, 1], 0, 0);
    payload.truncate(curve);
    head(&mut payload, 1, 12);
    payload.extend_from_slice(&1u16.to_le_bytes());
    payload.extend_from_slice(&1u16.to_le_bytes());
    payload.extend_from_slice(&1u32.to_le_bytes());
    payload.extend_from_slice(&F64_MINUS_ONE);
    payload.extend_from_slice(&1u16.to_le_bytes());
    payload.extend_from_slice(&0u16.to_le_bytes());
    payload.extend_from_slice(&0u16.to_le_bytes());
    for _ in 0..4 {
        payload.extend_from_slice(&(-2i32).to_le_bytes());
    }
    payload.extend_from_slice(&[0; 10]);
    let lists = SketchRecordLists::new(&payload);
    assert!(lists.listed_curve(&payload, curve).is_some());
    let (_, entity, losses) = projected_curve(payload.clone(), curve);
    let entity = entity.expect("the listed circle is projected");
    assert!(matches!(
        entity.geometry.definition(),
        SketchGeometryDefinition::Circle { .. }
    ));
    assert!(!entity.construction);
    assert!(losses.is_empty());
    let held = SketchGeometry::native(cadmpeg_core::nonblank_const!("sldprt:synthetic-circle"));
    for geometry in [&entity.geometry, &held] {
        assert_eq!(
            marker_is_construction(&payload, &lists, curve, geometry, None),
            (entity.construction, false)
        );
    }
}

#[test]
fn selector_twelve_roster_line_is_construction() {
    let (mut payload, curve) = sketch_with_line(CODE_FOUR, 12, [0, 1], 0, u32::MAX);
    let role = curve + crate::layout::current_extended_zero_tail_92_profile_curve::ROLE;
    payload[role..role + 2].copy_from_slice(&0u16.to_le_bytes());
    let lists = SketchRecordLists::new(&payload);
    let (_, entity, losses) = projected_curve(payload.clone(), curve);
    let entity = entity.expect("the listed line is projected");
    assert!(matches!(
        entity.geometry.definition(),
        SketchGeometryDefinition::Line { .. }
    ));
    assert!(entity.construction);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        SldprtLossCode::SketchListedCurveRelationDropped.kind()
    );
    assert_eq!(
        marker_is_construction(&payload, &lists, curve, &entity.geometry, None),
        (true, false)
    );
}

#[test]
fn selector_twelve_role_one_line_is_construction_on_both_paths() {
    for code in [CODE_FOUR, 2] {
        for (trailer, identity) in [(0, u32::MAX), (0, 7), (1, 7)] {
            let (mut payload, curve) = sketch_with_line(code, 12, [0, 1], trailer, identity);
            if code == 2 {
                let prefix = crate::resolved_features::LEGACY_EXTENDED_SKETCH_MARKER;
                payload[curve..curve + prefix.len()].copy_from_slice(prefix);
            }
            let lists = SketchRecordLists::new(&payload);
            let (_, entity, losses) = projected_curve(payload.clone(), curve);
            let entity = entity.expect("the listed line keeps an entity");
            assert!(entity.construction);
            assert_eq!(
                marker_is_construction(&payload, &lists, curve, &entity.geometry, None),
                (true, true)
            );
            assert_eq!(losses.len(), if code == CODE_FOUR { 2 } else { 1 });
            assert_eq!(
                losses
                    .iter()
                    .filter(
                        |loss| loss.code == SldprtLossCode::SketchListedCurveRelationDropped.kind()
                    )
                    .count(),
                usize::from(code == CODE_FOUR)
            );
            let loss = losses
                .iter()
                .find(|loss| {
                    loss.code == SldprtLossCode::SketchListedLineConstructionAssumed.kind()
                })
                .expect("construction assumption is reported");
            assert!(loss.message.contains(entity.sketch.as_str()));
            assert!(loss.message.contains("generated-profile"));
            assert!(loss
                .message
                .contains(entity.native_ref.as_deref().expect("marker reference")));
            assert!(loss.message.contains("selector bit 3"));
        }
    }
}

/// A curve that its `sgSketch` record gives takes its construction mark from
/// the record's selector, also where the marker layout of the same bytes
/// reads a selected construction line.
#[test]
fn a_record_read_curve_takes_construction_from_its_selector_alone() {
    let (mut payload, curve) = sketch_with_line(2, 4, [0, 1], 0, u32::MAX);
    let prefix = crate::resolved_features::LEGACY_EXTENDED_SKETCH_MARKER;
    payload[curve..curve + prefix.len()].copy_from_slice(prefix);
    let role = curve + crate::layout::current_extended_zero_tail_92_profile_curve::ROLE;
    payload[role..role + 2].copy_from_slice(&2u16.to_le_bytes());
    let lists = SketchRecordLists::new(&payload);
    let geometry = SketchGeometry::native(cadmpeg_core::nonblank_const!("sldprt:synthetic-line"));
    assert_eq!(
        marker_is_construction(&payload, &lists, curve, &geometry, None),
        (true, false)
    );
    assert_eq!(
        marker_is_construction(&payload, &lists, curve, &geometry, Some(false)),
        (false, false)
    );
    assert_eq!(
        marker_is_construction(&payload, &lists, curve, &geometry, Some(true)),
        (true, false)
    );
}
