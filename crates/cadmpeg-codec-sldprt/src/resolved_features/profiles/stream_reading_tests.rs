// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::features::{
    BooleanOp, Feature, FeatureDefinition, FeatureId, FeatureOperation, PartialRevolveConstruction,
    PlanarProfileRef, RevolveConstruction, SketchFeatureBinding,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchPlacement, SketchProfiles,
};

use super::{finalize_unread_marker_entities, project_marker_backed_sketches, HeldMarkerGeometry};
use crate::loss::SldprtLossCode;
use crate::records::{
    Feature as NativeFeature, FeatureHistory, FeatureInputLane, FeatureSource, SketchInputEntity,
    SketchInputKind,
};
use crate::test_support::sketch_records::framed_line_sketch;

const LANE: &str = "lane";
const NAME: &str = "generated-profile";
/// The sides of the box the marker records draw, in millimetres.
const BOX: [f64; 2] = [460.0, 60.0];

fn marker_sketch_id() -> SketchId {
    SketchId::mint(format!("sldprt:model:sketch#markers:{LANE}:1")).unwrap()
}

fn stream_id(stream: usize) -> SketchId {
    SketchId::mint(format!("sldprt:model:sketch#7:{stream}:0")).unwrap()
}

fn sketch_feature_id() -> FeatureId {
    FeatureId::mint("synthetic:test:id#feature").unwrap()
}

/// The point and curve markers of a framed `sgSketch` object that draws the
/// box `BOX`.
fn box_markers() -> (Vec<SketchInputEntity>, Vec<u8>) {
    let corners = [
        [0.0, 0.0],
        [BOX[0] / 1000.0, BOX[1] / 1000.0],
        [0.0, BOX[1] / 1000.0],
        [BOX[0] / 1000.0, 0.0],
    ];
    let (payload, points, lines) =
        framed_line_sketch(&corners, &[(0, 3, 4), (0, 2, 4), (2, 1, 4), (3, 1, 4)]);
    let marker =
        |id: String, ordinal: usize, offset: u64, kind, coordinates_m: Option<[f64; 2]>| {
            let mut marker =
                SketchInputEntity::new(&id, LANE, u32::try_from(ordinal).unwrap(), offset, kind);
            marker.feature_ref = Some("feature-native".into());
            marker.coordinates_m = coordinates_m.and_then(cadmpeg_ir::units::FiniteVector::new);
            marker
        };
    let mut markers = Vec::new();
    for (index, (offset, coordinates)) in points.iter().zip(corners).enumerate() {
        markers.push(marker(
            format!("corner{index}"),
            index,
            *offset,
            SketchInputKind::Point,
            Some(coordinates),
        ));
    }
    for (index, offset) in lines.iter().enumerate() {
        markers.push(marker(
            format!("edge{index}"),
            corners.len() + index,
            *offset,
            SketchInputKind::LineOrCircle,
            None,
        ));
    }
    (markers, payload)
}

fn entity(sketch: &SketchId, name: &str, geometry: SketchGeometryDefinition) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!(
            "synthetic:test:id#{}-{name}",
            sketch.as_str().rsplit_once('#').map_or("", |(_, key)| key)
        ))
        .unwrap(),
        sketch.clone(),
        SketchGeometry::try_from(geometry).unwrap(),
    )
}

fn circle(sketch: &SketchId, name: &str, radius: f64) -> SketchEntity {
    entity(
        sketch,
        name,
        SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(radius).unwrap(),
        },
    )
}

fn rectangle(sketch: &SketchId) -> Vec<SketchEntity> {
    let [width, height] = BOX;
    let corners = [
        Point2::new(0.0, 0.0),
        Point2::new(width, 0.0),
        Point2::new(width, height),
        Point2::new(0.0, height),
    ];
    (0..4)
        .map(|index| {
            entity(
                sketch,
                &format!("side{index}"),
                SketchGeometryDefinition::Line {
                    start: corners[index],
                    end: corners[(index + 1) % 4],
                },
            )
        })
        .collect()
}

/// A profile-stream sketch of `entities`, one loop per group.
fn stream_sketch(
    id: &SketchId,
    name: Option<&str>,
    loops: &[&[SketchEntity]],
) -> (Sketch, Vec<SketchEntity>) {
    let profiles = loops
        .iter()
        .map(|entities| {
            entities
                .iter()
                .map(|entity| SketchEntityUse {
                    entity: entity.id().clone(),
                    reversed: false,
                })
                .collect()
        })
        .collect::<Vec<_>>();
    let sketch = Sketch {
        id: id.clone(),
        name: name.map(str::to_owned),
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: SketchProfiles::try_from(profiles).unwrap(),
        native_ref: Some(LANE.into()),
    };
    (
        sketch,
        loops
            .iter()
            .flat_map(|entities| entities.iter().cloned())
            .collect(),
    )
}

fn feature(id: FeatureId, native: &str, name: &str, definition: FeatureDefinition) -> Feature {
    Feature {
        product_definition: None,
        id,
        ordinal: 0,
        name: Some(name.into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
        native_ref: Some(native.into()),
    }
}

struct Decoded {
    features: Vec<Feature>,
    sketches: Vec<Sketch>,
    losses: Vec<LossNote>,
}

/// Runs the marker pass, the finalize pass and the consumer binding over one
/// sketch feature bound to the profile-stream sketch `bound`, whose marker
/// records draw the box, and one revolve that consumes the sketch feature.
fn decode(streams: Vec<(Sketch, Vec<SketchEntity>)>, bound: &SketchId) -> Decoded {
    decode_markers(streams, bound, box_markers())
}

/// `decode` with the marker records `markers` in place of the box's.
fn decode_markers(
    streams: Vec<(Sketch, Vec<SketchEntity>)>,
    bound: &SketchId,
    (markers, payload): (Vec<SketchInputEntity>, Vec<u8>),
) -> Decoded {
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
            name: NAME.into(),
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
    let mut features = vec![
        feature(
            sketch_feature_id(),
            "feature-native",
            NAME,
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(bound.clone())),
            }),
        ),
        feature(
            FeatureId::mint("synthetic:test:id#revolve").unwrap(),
            "revolve-native",
            "revolve",
            FeatureDefinition::Operation(FeatureOperation::Revolve {
                construction: RevolveConstruction::Unresolved(PartialRevolveConstruction::Axis {
                    profile: PlanarProfileRef::Feature(sketch_feature_id()),
                    extent: None,
                    solid: Some(true),
                    face_maker: None,
                    fuse_order: None,
                    allow_multi_profile_faces: None,
                }),
                op: BooleanOp::Join,
            }),
        ),
    ];
    let lanes = vec![FeatureInputLane {
        id: LANE.into(),
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
    let (mut sketches, entities): (Vec<_>, Vec<Vec<_>>) = streams.into_iter().unzip();
    let mut entities = entities.into_iter().flatten().collect::<Vec<_>>();
    let mut held = HeldMarkerGeometry::default();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    project_marker_backed_sketches(
        &ctx,
        &mut features,
        &mut sketches,
        &mut entities,
        &mut held,
        &histories,
        &lanes,
    )
    .unwrap();
    let mut losses = Vec::new();
    finalize_unread_marker_entities(&mut entities, held, &mut losses);
    crate::history::bind::bind_bound_sketch_profiles(&mut features, &sketches);
    Decoded {
        features,
        sketches,
        losses,
    }
}

fn bound_sketch(features: &[Feature]) -> Option<&SketchId> {
    match features[0].evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::Sketch { sketch }) => sketch.id(),
        _ => None,
    }
}

fn revolve_profile(features: &[Feature]) -> Option<&PlanarProfileRef> {
    match features[1].evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::Revolve { construction, .. }) => {
            construction.profile()
        }
        _ => None,
    }
}

fn second_readings(losses: &[LossNote]) -> Vec<&str> {
    losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::SketchSecondReading.kind())
        .map(|loss| loss.message.as_str())
        .collect()
}

/// A sketch feature whose own marker records close a profile, bound to a
/// one-circle profile-stream sketch that carries its name, binds the marker
/// sketch, and the revolve that consumes it revolves the marker sketch. The
/// stream sketch stays in the model with its name and its circle.
#[test]
fn a_feature_bound_to_a_stream_circle_binds_the_profile_of_its_own_records() {
    let circle_id = stream_id(0);
    let circle = circle(&circle_id, "circle", 40.0);
    let decoded = decode(
        vec![stream_sketch(&circle_id, Some(NAME), &[&[circle]])],
        &circle_id,
    );
    assert_eq!(bound_sketch(&decoded.features), Some(&marker_sketch_id()));
    assert_eq!(
        revolve_profile(&decoded.features),
        Some(&PlanarProfileRef::Sketch(marker_sketch_id()))
    );
    let marker = decoded
        .sketches
        .iter()
        .find(|sketch| sketch.id == marker_sketch_id())
        .expect("the marker sketch is read");
    assert_eq!(marker.profiles.as_slice().len(), 1);
    let stream = decoded
        .sketches
        .iter()
        .find(|sketch| sketch.id == circle_id)
        .expect("the stream sketch stays");
    assert_eq!(stream.name.as_deref(), Some(NAME));
    assert!(second_readings(&decoded.losses).is_empty());
}

/// The radius-signature step left the feature's own interval-bound stream
/// sketch, the box, unbound with the feature's name and bound the feature to
/// another feature's stream circle. The feature binds the box its marker
/// records draw; the unbound box is named a second reading of it, and the
/// circle is no reading of it.
#[test]
fn the_stream_box_a_signature_binding_left_unbound_is_a_second_reading() {
    let circle_id = stream_id(0);
    let box_id = stream_id(1);
    let circle = circle(&circle_id, "circle", 12.0);
    let sides = rectangle(&box_id);
    let decoded = decode(
        vec![
            stream_sketch(&circle_id, Some(NAME), &[&[circle]]),
            stream_sketch(&box_id, Some(NAME), &[&sides]),
        ],
        &circle_id,
    );
    assert_eq!(bound_sketch(&decoded.features), Some(&marker_sketch_id()));
    let readings = second_readings(&decoded.losses);
    assert_eq!(readings.len(), 1, "{readings:?}");
    assert!(readings[0].starts_with(&format!("sketch {box_id}:")));
    assert!(readings[0].contains(marker_sketch_id().as_str()));
    assert!(decoded.sketches.iter().any(|sketch| sketch.id == box_id));
    assert!(decoded.sketches.iter().any(|sketch| sketch.id == circle_id));
}

/// A bound stream sketch that is no lone circle keeps the binding, also
/// where the marker records draw the same box, and the marker pass adds no
/// sketch.
#[test]
fn a_bound_stream_box_keeps_the_binding_beside_the_marker_box() {
    let box_id = stream_id(0);
    let sides = rectangle(&box_id);
    let decoded = decode(vec![stream_sketch(&box_id, None, &[&sides])], &box_id);
    assert_eq!(bound_sketch(&decoded.features), Some(&box_id));
    assert!(decoded
        .sketches
        .iter()
        .all(|sketch| sketch.id != marker_sketch_id()));
    assert!(second_readings(&decoded.losses).is_empty());
}

/// A marker sketch with an entity that has no planar geometry does not take
/// the binding from a stream circle.
#[test]
fn a_marker_sketch_with_an_unread_handle_leaves_the_stream_circle_bound() {
    let circle_id = stream_id(0);
    let circle = circle(&circle_id, "circle", 40.0);
    let (mut markers, mut payload) = box_markers();
    let stray = u64::try_from(payload.len()).unwrap();
    payload.extend([0; 16]);
    let mut handle =
        SketchInputEntity::new("stray", LANE, 99, stray, SketchInputKind::LineOrCircle);
    handle.feature_ref = Some("feature-native".into());
    markers.push(handle);
    let decoded = decode_markers(
        vec![stream_sketch(&circle_id, Some(NAME), &[&[circle]])],
        &circle_id,
        (markers, payload),
    );
    assert_eq!(bound_sketch(&decoded.features), Some(&circle_id));
}

/// A stream sketch that closes more loops than the feature's marker records
/// and repeats none of them keeps the binding, and the marker pass adds no
/// sketch.
#[test]
fn a_stream_sketch_with_more_loops_than_the_marker_records_keeps_the_binding() {
    let stream = stream_id(0);
    let sides = rectangle(&stream);
    let hole = circle(&stream, "hole", 5.0);
    let decoded = decode(
        vec![stream_sketch(&stream, Some(NAME), &[&sides, &[hole]])],
        &stream,
    );
    assert_eq!(bound_sketch(&decoded.features), Some(&stream));
    assert!(decoded
        .sketches
        .iter()
        .all(|sketch| sketch.id != marker_sketch_id()));
    assert!(second_readings(&decoded.losses).is_empty());
}
