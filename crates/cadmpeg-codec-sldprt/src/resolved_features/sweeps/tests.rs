use super::*;
use crate::records::{
    FeatureInputClass, FeatureInputName, FeatureSource, ObjectId, SketchInputEntity,
    SketchInputKind,
};
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::profiles::PlanarProfileRef;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{FeatureContent, FeatureId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{SketchId, SketchPlacement, SketchProfiles, SpatialSketchId};
use std::collections::BTreeMap;

pub(super) fn lane(payload: Vec<u8>) -> FeatureInputLane {
    FeatureInputLane {
        id: "lane#1".into(),
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
        sketch_entities: Vec::new(),
    }
}

pub(super) fn class(bytes: &mut Vec<u8>, name: &str) -> usize {
    let offset = bytes.len();
    bytes.extend_from_slice(&[0xff, 0xff, 1, 0]);
    bytes.extend_from_slice(&u16::try_from(name.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(name.as_bytes());
    offset
}

pub(super) fn operand(
    bytes: &mut Vec<u8>,
    circular: bool,
    repeated: bool,
) -> (usize, usize, usize) {
    let contact = class(bytes, "moWireContact_c");
    for value in [0.0f64, 0.0, 1.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0u32, 1, u32::MAX] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&2u16.to_le_bytes());
    for value in [0.0f64, 0.0, -1.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    if repeated {
        bytes.extend_from_slice(&[0x23, 0x81, 0x25, 0x81]);
    } else {
        class(bytes, "moGeneralCurveRef_w");
        class(bytes, "moCompProfile_c");
    }
    let prefix = bytes.len();
    bytes.extend_from_slice(&[0x2b, 0x80, 2, 0, 0, 0, 0, 0, 0, 0]);
    bytes.resize(prefix + 81 + 52 + 33, 0);
    bytes[prefix + 45..prefix + 61].fill(0xff);
    let source = prefix + 81;
    bytes[source..source + 4].copy_from_slice(&11u32.to_le_bytes());
    bytes[source + 4..source + 8].copy_from_slice(&7u32.to_le_bytes());
    for at in [32, 36, 40] {
        bytes[source + at..source + at + 4].copy_from_slice(&(-12345i32).to_le_bytes());
    }
    bytes[source + 48..source + 52].copy_from_slice(&16000u32.to_le_bytes());
    let tail = source + 52;
    bytes[tail + 12..tail + 16].fill(0xff);
    bytes[tail + 24..tail + 32].copy_from_slice(&1.0f64.to_le_bytes());
    bytes.extend_from_slice(&[0xff; 8]);
    bytes.extend_from_slice(&[0; 6]);
    let options = bytes.len();
    bytes.resize(options + 105, 0);
    if circular {
        bytes[options + 86..options + 90].copy_from_slice(&[1, 0, 0, 0]);
        bytes[options + 90..options + 98].copy_from_slice(&0.008f64.to_le_bytes());
    }
    (contact, source, options)
}

pub(super) fn head(bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(&[0xff, 0xff, 0x1f, 0, 3]);
    bytes.extend_from_slice(&[0xff; 8]);
    bytes.extend_from_slice(&(-1.0f32).to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0, 0, 4, 0, 2, 0, 1, 0, 0, 0]);
    bytes.extend_from_slice(&(-1.0f32).to_le_bytes());
    bytes.extend_from_slice(&[0, 0, 4, 0]);
    bytes.extend_from_slice(&[0; 9]);
    bytes.extend_from_slice(&1.0f64.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
}

pub(super) fn sketch(spatial: bool) -> FeatureInputLane {
    let mut bytes = vec![0; 32];
    let points: &[[f64; 3]] = if spatial {
        &[[0.0, 0.0, 0.0], [0.012, 0.0, 0.0], [0.012, 0.014, 0.009]]
    } else {
        &[
            [0.0, 0.0, 0.0],
            [0.012, 0.006, 0.0],
            [0.012, 0.0, 0.0],
            [0.018, 0.006, 0.0],
            [0.018, 0.022, 0.0],
        ]
    };
    bytes.extend_from_slice(&u32::try_from(points.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    for point in points {
        head(&mut bytes);
        bytes.extend_from_slice(&(if spatial { 0x0eu16 } else { 0x1e }).to_le_bytes());
        for value in &point[..if spatial { 3 } else { 2 }] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&(-2i32).to_le_bytes());
        bytes.extend_from_slice(&[0; 46]);
    }
    bytes.extend_from_slice(&[0; 4]);
    let definitions: &[(u16, u16, Option<u16>)] = if spatial {
        &[(1, 2, None), (0, 1, None)]
    } else {
        &[(0, 2, None), (3, 4, None), (2, 3, Some(1))]
    };
    let mut offsets = Vec::new();
    for &(start, end, center) in definitions {
        offsets.push(bytes.len());
        head(&mut bytes);
        bytes.extend_from_slice(&start.to_le_bytes());
        bytes.extend_from_slice(&end.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(-1.0f64).to_le_bytes());
        bytes.extend_from_slice(&i32::from(center.is_some()).to_le_bytes());
        if let Some(center) = center {
            bytes.extend_from_slice(&center.to_le_bytes());
            for _ in 0..4 {
                bytes.extend_from_slice(&(-2i32).to_le_bytes());
            }
        }
        if center.is_some() {
            bytes.extend_from_slice(&[0; 10]);
        } else {
            bytes.extend_from_slice(&[0; 8]);
        }
    }
    let mut lane = lane(bytes);
    for (ordinal, offset) in offsets.into_iter().enumerate() {
        let mut marker = SketchInputEntity::new(
            format!("marker#{offset}"),
            "lane#1",
            u32::try_from(ordinal).unwrap(),
            u64::try_from(offset).unwrap(),
            SketchInputKind::LineOrCircle,
        );
        marker.feature_ref = Some("source".into());
        lane.sketch_entities.push(marker);
    }
    lane
}

/// Removes the `sgSketch` frame of the [`sketch`]`(true)` records at the start
/// of `lane`: the point count becomes `0`, the line indices become one-based
/// positions, and each point record becomes a primary point marker.
pub(super) fn unframe(lane: &mut FeatureInputLane) {
    use super::super::sketch_lists::{ListedRecord, SketchRecordLists};
    use cadmpeg_core::decode::View;
    let lists = SketchRecordLists::new(&lane.native_payload);
    let offset = usize::try_from(lane.sketch_entities[0].offset()).unwrap();
    let Some(ListedRecord::Curve { sketch, .. }) = lists.record(offset) else {
        panic!("curve")
    };
    let points = (0..3)
        .map(|index| sketch.point(index).unwrap())
        .collect::<Vec<_>>();
    let mut payload = lane.native_payload.to_vec();
    payload[32..36].fill(0);
    for marker in &lane.sketch_entities {
        let offset = usize::try_from(marker.offset()).unwrap();
        for field in [64, 66] {
            let index = View::u16_le_at(&payload, offset + field).unwrap() + 1;
            payload[offset + field..offset + field + 2].copy_from_slice(&index.to_le_bytes());
        }
    }
    lane.native_payload = payload.into();
    for (ordinal, offset) in points.into_iter().enumerate() {
        let mut marker = SketchInputEntity::new(
            format!("point#{offset}"),
            "lane#1",
            u32::try_from(ordinal).unwrap(),
            u64::try_from(offset).unwrap(),
            SketchInputKind::Point,
        );
        marker.feature_ref = Some("source".into());
        lane.sketch_entities.push(marker);
    }
}

fn native(id: &str, source: u32, class: &str) -> crate::records::Feature {
    crate::records::Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(source),
        ordinal: source,
        name: id.into(),
        kind: "Feature".into(),
        input_class: Some(class.into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    }
}

fn feature(id: &str, definition: FeatureDefinition) -> Feature {
    Feature {
        id: FeatureId::mint(format!("test:model:feature#{id}")).unwrap(),
        ordinal: u64::from(id != "source"),
        product_definition: None,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        native_ref: Some(id.into()),
        evaluation: FeatureEvaluation::from_definition(definition),
    }
}

struct Input {
    features: Vec<Feature>,
    history: FeatureHistory,
    lane: FeatureInputLane,
    source: usize,
    sketches: Vec<Sketch>,
    entities: Vec<SketchEntity>,
    spatial_entities: Vec<SpatialSketchEntity>,
}

fn input(spatial: bool, circular: bool) -> Input {
    let mut lane = sketch(spatial);
    let consumer = lane.native_payload.len();
    let mut bytes = lane.native_payload.to_vec();
    bytes.extend_from_slice(&[0; 32]);
    let (contact, source, _) = operand(&mut bytes, circular, false);
    lane.native_payload = bytes.into();
    lane.classes.push(FeatureInputClass {
        id: "contact".into(),
        parent: lane.id.clone(),
        ordinal: 0,
        offset: u64::try_from(contact).unwrap(),
        name: "moWireContact_c".into(),
    });
    for (id, source, offset) in [("source", 11, 16), ("sweep", 31, consumer)] {
        lane.names.push(FeatureInputName {
            id: id.into(),
            parent: lane.id.clone(),
            ordinal: 0,
            offset: u64::try_from(offset).unwrap(),
            object_id: ObjectId::from_value(source),
            value: id.into(),
        });
    }
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native(
                "source",
                11,
                if spatial {
                    "mo3DProfileFeature_c"
                } else {
                    "moProfileFeature_c"
                },
            ),
            native("sweep", 31, "moSweep_c"),
        ],
    };
    let mut sketches = Vec::new();
    let source_definition = if spatial {
        FeatureOperation::SpatialSketch {
            sketch: Some(SpatialSketchId::mint("test:model:spatial-sketch#source").unwrap()),
        }
    } else {
        let id = SketchId::mint("test:model:sketch#source").unwrap();
        sketches.push(Sketch {
            id: id.clone(),
            name: None,
            configuration: None,
            visible: None,
            placement: SketchPlacement::try_resolved(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
            profiles: SketchProfiles::default(),
            native_ref: None,
        });
        FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Planar(Some(id)),
        }
    };
    let mode = if circular {
        cadmpeg_ir::features::sweeps::SweepMode::Unresolved {}
    } else {
        cadmpeg_ir::features::sweeps::SweepMode::Solid {
            op: SolidSweepOperation::Join,
        }
    };
    let features = vec![
        feature("source", FeatureDefinition::Operation(source_definition)),
        feature(
            "sweep",
            FeatureDefinition::Operation(FeatureOperation::Sweep {
                shape: SweepShape::sheet_sections(
                    mode,
                    SweepSection::Profile(PlanarProfileRef::Native("authored-section".into())),
                    Vec::new(),
                ),
                path: None,
                orientation: None,
                transition: None,
                transformation: None,
                path_tangent: false,
                linearize: false,
                twist: None,
                path_extent: None,
                guide_rail: None,
                taper: None,
                scale: None,
                allow_multi_profile_faces: None,
            }),
        ),
    ];
    Input {
        features,
        history,
        lane,
        source,
        sketches,
        entities: Vec::new(),
        spatial_entities: Vec::new(),
    }
}

fn apply(input: &mut Input) -> Vec<LossNote> {
    let mut losses = Vec::new();
    bind(
        &mut input.features,
        std::slice::from_ref(&input.history),
        std::slice::from_ref(&input.lane),
        Geometry {
            sketches: &input.sketches,
            entities: &mut input.entities,
            spatial_entities: &mut input.spatial_entities,
        },
        &mut losses,
    );
    losses
}

#[test]
fn circular_spatial_path_binds_curves_and_keeps_diameter() {
    let mut input = input(true, true);
    let losses = apply(&mut input);
    let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, path, .. }) =
        input.features[1].evaluation.definition()
    else {
        panic!("sweep")
    };
    let Some(GeneratedSweepSection::CircularRegion { region }) = shape.generated_section() else {
        panic!("circular section")
    };
    assert_eq!(region.outer_radius().get(), 4.0);
    assert!(matches!(path, Some(PathRef::SpatialSketchCurves { curves, .. }) if curves.len() == 2));
    assert_eq!(input.spatial_entities.len(), 2);
    assert!(input.features[1]
        .dependencies
        .contains(&input.features[0].id));
    assert!(losses
        .iter()
        .any(|note| note.code == SldprtLossCode::FeatureSweepMergeAssumed.kind()));
}

#[test]
fn planar_path_keeps_its_independent_section() {
    let mut input = input(false, false);
    apply(&mut input);
    let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, path, .. }) =
        input.features[1].evaluation.definition()
    else {
        panic!("sweep")
    };
    assert!(matches!(path, Some(PathRef::SketchCurves { curves, .. }) if curves.len() == 3));
    assert!(
        matches!(shape.referenced_profile(), Some(PlanarProfileRef::Native(section)) if section == "authored-section")
    );
    assert_eq!(input.entities.len(), 3);
    assert!(input.features[1]
        .dependencies
        .contains(&input.features[0].id));
}

#[test]
fn malformed_source_is_a_native_path_loss() {
    let mut input = input(false, false);
    input.lane.native_payload[input.source + 32] = 0;
    let losses = apply(&mut input);
    assert!(matches!(
        input.features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sweep {
            path: Some(PathRef::Native(_)),
            ..
        })
    ));
    assert!(losses
        .iter()
        .any(|note| note.code == SldprtLossCode::FeatureSweepPathRetained.kind()));
    assert!(input.entities.is_empty());
}

#[test]
fn a_duplicate_producer_keeps_its_native_path() {
    let mut input = input(false, false);
    input
        .history
        .features
        .push(native("duplicate", 11, "moProfileFeature_c"));
    apply(&mut input);
    assert!(matches!(
        input.features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sweep {
            path: Some(PathRef::Native(_)),
            ..
        })
    ));
    assert!(input.features[1].dependencies.is_empty());
}

#[test]
fn an_unreadable_wire_contact_is_a_loss() {
    let mut input = input(false, false);
    let offset = usize::try_from(input.lane.classes[0].offset).unwrap();
    let body = offset + 6 + "moWireContact_c".len();
    input.lane.native_payload[body + 24..body + 28].copy_from_slice(&7u32.to_le_bytes());
    let losses = apply(&mut input);
    assert!(matches!(
        input.features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sweep { path: None, .. })
    ));
    assert!(losses
        .iter()
        .any(|note| note.code == SldprtLossCode::FeatureSweepPathRetained.kind()));
    assert!(input.entities.is_empty());
}

#[test]
fn an_extended_path_header_has_a_pinned_loss() {
    let mut input = input(false, false);
    let mut payload = input.lane.native_payload.to_vec();
    payload.splice(input.source..input.source, 17000u32.to_le_bytes());
    input.lane.native_payload = payload.into();
    let losses = apply(&mut input);
    assert!(matches!(
        input.features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sweep {
            path: Some(PathRef::SketchCurves { .. }),
            ..
        })
    ));
    assert!(losses
        .iter()
        .any(|note| note.code == SldprtLossCode::FeatureSweepPathHeaderAssumed.kind()));
}

#[test]
fn configuration_geometry_must_agree_before_global_path_binding() {
    use crate::resolved_features::sketch_lists::{ListedRecord, SketchRecordLists};
    let mut input = input(true, true);
    let mut other = input.lane.clone();
    other.id = "lane#2".into();
    other.configuration = Some("other".into());
    for class in &mut other.classes {
        class.parent = other.id.clone();
    }
    for name in &mut other.names {
        name.parent = other.id.clone();
    }
    for marker in &mut other.sketch_entities {
        marker.set_test_parent(other.id.clone());
        marker.set_test_id(format!("other:{}", marker.id()));
    }
    let lists = SketchRecordLists::new(&other.native_payload);
    let offset = usize::try_from(other.sketch_entities[0].offset()).unwrap();
    let Some(ListedRecord::Curve { sketch, .. }) = lists.record(offset) else {
        panic!("curve")
    };
    let point = sketch.point(2).unwrap();
    other.native_payload[point + 82..point + 90].copy_from_slice(&0.010f64.to_le_bytes());
    let mut losses = Vec::new();
    bind(
        &mut input.features,
        &[input.history],
        &[input.lane, other],
        Geometry {
            sketches: &input.sketches,
            entities: &mut input.entities,
            spatial_entities: &mut input.spatial_entities,
        },
        &mut losses,
    );
    assert!(matches!(
        input.features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sweep {
            path: Some(PathRef::Native(_)),
            ..
        })
    ));
    assert!(input.spatial_entities.is_empty());
    assert!(losses
        .iter()
        .any(|note| note.code == SldprtLossCode::FeatureSweepPathRetained.kind()));
}

#[test]
fn an_existing_path_keeps_its_operand() {
    let mut input = input(false, false);
    input.features[1].evaluation.edit(|definition, _| {
        if let FeatureDefinition::Operation(FeatureOperation::Sweep { path, .. }) = definition {
            *path = Some(PathRef::Native("authored-path".into()));
        }
    });
    assert!(apply(&mut input).is_empty());
    assert!(input.entities.is_empty());
    assert!(
        matches!(input.features[1].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sweep { path: Some(PathRef::Native(path)), .. }) if path == "authored-path")
    );
}

fn set(input: &mut Input, marker: usize, at: usize, bytes: &[u8]) {
    let offset = usize::try_from(input.lane.sketch_entities[marker].offset()).unwrap() + at;
    let mut payload = input.lane.native_payload.to_vec();
    payload[offset..offset + bytes.len()].copy_from_slice(bytes);
    input.lane.native_payload = payload.into();
}

fn bound_path(input: &Input) -> Option<&PathRef> {
    let FeatureDefinition::Operation(FeatureOperation::Sweep { path, .. }) =
        input.features[1].evaluation.definition()
    else {
        panic!("sweep")
    };
    path.as_ref()
}

fn charged(losses: &[LossNote], code: SldprtLossCode) -> bool {
    losses.iter().any(|note| note.code == code.kind())
}

const GEOMETRY_LOCUS: [u8; 4] = [0x05, 0x00, 0x01, 0x00];

/// `docs/formats/sldprt.md` §2.3: a line with an identifier trailer off locus
/// `05 00 01 00` indexes the point records of its sketch by position, like
/// any other line, so the path states no assumed reading.
#[test]
fn an_identifier_trailer_path_line_reads_its_point_record_positions() {
    let mut input = input(false, false);
    set(
        &mut input,
        0,
        crate::layout::sketch_line_record::TRAILER,
        &1u32.to_le_bytes(),
    );
    let losses = apply(&mut input);
    assert!(matches!(
        bound_path(&input),
        Some(PathRef::SketchCurves { .. })
    ));
    for code in [
        SldprtLossCode::FeatureSweepPathRetained,
        SldprtLossCode::FeatureSweepPathRosterAssumed,
        SldprtLossCode::SketchPointRecordIndexBasisAssumed,
    ] {
        assert!(!charged(&losses, code), "{code:?}");
    }
}

/// Open item EV-27: what the indices of a curve record on locus
/// `05 00 01 00` select is not established, so a planar path that reads
/// them by point-record position states that reading.
#[test]
fn a_planar_path_arc_on_the_geometry_locus_charges_the_index_basis_assumption() {
    let mut input = input(false, false);
    set(
        &mut input,
        2,
        crate::layout::sketch_record_head::LOCUS,
        &GEOMETRY_LOCUS,
    );
    let losses = apply(&mut input);
    assert!(matches!(
        bound_path(&input),
        Some(PathRef::SketchCurves { curves, .. }) if curves.len() == 3
    ));
    assert!(charged(
        &losses,
        SldprtLossCode::SketchPointRecordIndexBasisAssumed
    ));
}

/// Open item EV-27, for a line of a framed spatial sketch. The word at +68
/// is not `1`, so the line is not the object-index line that §2 reads.
#[test]
fn a_spatial_path_line_on_the_geometry_locus_charges_the_index_basis_assumption() {
    let mut input = input(true, false);
    set(
        &mut input,
        0,
        crate::layout::sketch_record_head::LOCUS,
        &GEOMETRY_LOCUS,
    );
    set(&mut input, 0, 68, &0u32.to_le_bytes());
    let losses = apply(&mut input);
    assert!(matches!(
        bound_path(&input),
        Some(PathRef::SpatialSketchCurves { curves, .. }) if curves.len() == 2
    ));
    assert!(charged(
        &losses,
        SldprtLossCode::SketchPointRecordIndexBasisAssumed
    ));
    assert!(!charged(
        &losses,
        SldprtLossCode::FeatureSweepPathRosterAssumed
    ));
}

/// `docs/formats/sldprt.md` §2: an unframed spatial path line with an
/// identifier trailer is read by one-based primary-point position, which no
/// file establishes for that record form; a line without the trailer is not.
#[test]
fn an_unframed_spatial_trailer_line_charges_the_roster_assumption() {
    let mut plain = input(true, false);
    unframe(&mut plain.lane);
    let losses = apply(&mut plain);
    assert!(matches!(
        bound_path(&plain),
        Some(PathRef::SpatialSketchCurves { .. })
    ));
    assert!(!charged(
        &losses,
        SldprtLossCode::FeatureSweepPathRosterAssumed
    ));
    let mut input = input(true, false);
    unframe(&mut input.lane);
    set(
        &mut input,
        0,
        crate::layout::sketch_line_record::TRAILER,
        &1u32.to_le_bytes(),
    );
    let losses = apply(&mut input);
    assert!(matches!(
        bound_path(&input),
        Some(PathRef::SpatialSketchCurves { curves, .. }) if curves.len() == 2
    ));
    assert!(charged(
        &losses,
        SldprtLossCode::FeatureSweepPathRosterAssumed
    ));
    assert!(!charged(
        &losses,
        SldprtLossCode::SketchPointRecordIndexBasisAssumed
    ));
}

#[test]
fn a_represented_path_curve_keeps_its_model_identity() {
    let mut input = input(false, false);
    let first = paths::planar(&input.lane, "source").unwrap().remove(0);
    let id = SketchEntityId::mint("test:model:sketch-entity#represented").unwrap();
    input.entities.push(
        SketchEntity::new(id.clone(), input.sketches[0].id.clone(), first.geometry)
            .with_native_ref(Some(first.native)),
    );
    apply(&mut input);
    assert_eq!(input.entities.len(), 3);
    assert!(
        matches!(input.features[1].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sweep { path: Some(PathRef::SketchCurves { curves, .. }), .. }) if curves.first() == Some(&id))
    );
}

#[test]
fn a_solid_shape_with_a_circular_region_charges_the_merge_assumption() {
    let mut input = input(false, true);
    input.features[1].evaluation.edit(|definition, _| {
        if let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) = definition {
            *shape = SweepShape::Solid {
                op: SolidSweepOperation::Join,
                section: SweepSection::Profile(PlanarProfileRef::Native("authored-section".into())),
                sections: Vec::new(),
            };
        }
    });
    let losses = apply(&mut input);
    assert!(losses
        .iter()
        .any(|note| note.code == SldprtLossCode::FeatureSweepMergeAssumed.kind()));
}

#[test]
fn a_declared_reference_curve_path_keeps_its_native_path_and_applies_the_circular_section() {
    let mut input = input(false, true);
    let native = "sldprt:feature-input:component-reference-curve:1:0";
    input.features[1].evaluation.edit(|definition, _| {
        if let FeatureDefinition::Operation(FeatureOperation::Sweep { path, .. }) = definition {
            *path = Some(PathRef::Native(native.into()));
        }
    });
    let losses = apply(&mut input);
    let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, path, .. }) =
        input.features[1].evaluation.definition()
    else {
        panic!("sweep")
    };
    let Some(GeneratedSweepSection::CircularRegion { region }) = shape.generated_section() else {
        panic!("circular section")
    };
    assert_eq!(region.outer_radius().get(), 4.0);
    assert!(matches!(path, Some(PathRef::Native(path)) if path == native));
    assert!(input.entities.is_empty());
    assert!(losses
        .iter()
        .any(|note| note.code == SldprtLossCode::FeatureSweepMergeAssumed.kind()));
}
