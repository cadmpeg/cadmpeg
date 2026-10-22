use std::collections::HashMap;

use cadmpeg_ir::attributes::AttributeValue;
use cadmpeg_ir::features::{PlanarProfileRef, SketchProfileRegion};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchPlacement, SketchProfiles,
};

use super::{edge_name, EdgeName, ProfileBody, ProfileEdge, ProfileIndex, ProfileWire};
use crate::feature::{PmDcKernelEntityLink, PmDcKernelEntityLinkPayload, KERNEL_ENTITY_LINK_TYPE};
use crate::pmdc::PmDcReference;
use crate::record_identity::{Located, RecordTypeId};

const NAMING_TYPE: u32 = 38;
const SKETCH_ORDINAL: u32 = 2;
const BODY_ORDINAL: u32 = 30;

fn naming_values(naming_type: i64, associative_id: i64, topology: i64) -> Vec<AttributeValue> {
    let mut values = vec![
        AttributeValue::Reference("sat:brep:entity#-1".into()),
        AttributeValue::Integer(-1),
        AttributeValue::Reference("sat:brep:entity#-1".into()),
        AttributeValue::Reference("sat:brep:entity#-1".into()),
        AttributeValue::Reference("sat:brep:entity#9".into()),
    ];
    values.extend([naming_type, 0, associative_id, 0, topology].map(AttributeValue::Integer));
    values
}

#[test]
fn edge_naming_attribute_yields_its_naming_type_and_associative_id() {
    assert_eq!(
        edge_name(&naming_values(38, 14, 2)),
        Some(EdgeName {
            naming_type: 38,
            associative_id: 14,
        })
    );
    // A vertex name and a truncated attribute name no sketch curve.
    assert_eq!(edge_name(&naming_values(38, 14, 1)), None);
    assert_eq!(edge_name(&naming_values(38, 14, 2)[..9]), None);
}

fn sketch_id() -> SketchId {
    SketchId::mint("inventor:design:sketch#segment-2").expect("sketch id")
}

fn entity(ordinal: u32, definition: SketchGeometryDefinition) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("inventor:design:sketch-entity#segment-{ordinal}"))
            .expect("entity id"),
        sketch_id(),
        SketchGeometry::try_from(definition).expect("sketch geometry"),
    )
    .with_native_ref(Some(format!(
        "inventor:pmdc:sketch-entity#segment-{ordinal}"
    )))
}

/// A 20 mm square from the origin with a 3 mm circle at its center, on the
/// model XY plane. Square sides are entities 11 through 14; the circle is 15.
fn square_with_hole() -> (Sketch, Vec<SketchEntity>) {
    let corners = [(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)];
    let mut entities = (0..4)
        .map(|side| {
            let (start, end) = (corners[side], corners[(side + 1) % 4]);
            entity(
                11 + u32::try_from(side).expect("side fits"),
                SketchGeometryDefinition::Line {
                    start: Point2::new(start.0, start.1),
                    end: Point2::new(end.0, end.1),
                },
            )
        })
        .collect::<Vec<_>>();
    entities.push(entity(
        15,
        SketchGeometryDefinition::Circle {
            center: Point2::new(10.0, 10.0),
            radius: Length::new(3.0).expect("radius"),
        },
    ));
    let uses = |range: std::ops::Range<usize>| {
        entities[range]
            .iter()
            .map(|entity| SketchEntityUse {
                entity: entity.id().clone(),
                reversed: false,
            })
            .collect::<Vec<_>>()
    };
    let sketch = Sketch {
        id: sketch_id(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("placement"),
        profiles: SketchProfiles::try_from(vec![uses(0..4), uses(4..5)]).expect("profiles"),
        native_ref: Some("inventor:pmdc:sketch#segment-2".into()),
    };
    (sketch, entities)
}

fn edge(associative_id: i64, start: (f64, f64), end: (f64, f64)) -> ProfileEdge {
    ProfileEdge {
        name: Some(EdgeName {
            naming_type: i64::from(NAMING_TYPE),
            associative_id,
        }),
        start: Point3::new(start.0, start.1, 0.0),
        end: Point3::new(end.0, end.1, 0.0),
    }
}

fn kernel_link(ordinal: u32, entity_id: u32) -> PmDcKernelEntityLink {
    Located::new(
        PmDcKernelEntityLinkPayload {
            save_version_major: 22,
            header_value: 0,
            header_id: 0,
            selection_source: 0,
            state: 4,
            body: PmDcReference::new(BODY_ORDINAL + 1, true).expect("body reference"),
            entity_id,
            entity_type: NAMING_TYPE,
        },
        RecordTypeId::from_bytes(KERNEL_ENTITY_LINK_TYPE),
        cadmpeg_ir::identity_key!("segment"),
        ordinal,
    )
}

/// Profile wires for the square (key 7) and the circle (key 8). The square
/// wire starts its sides at `square_start`.
fn profile_body(square_start: (f64, f64)) -> ProfileBody {
    ProfileBody {
        segment_token: cadmpeg_ir::identity_key!("segment"),
        record_ordinal: BODY_ORDINAL,
        wires: vec![
            ProfileWire {
                key: 7,
                edges: vec![
                    edge(1, square_start, (20.0, 0.0)),
                    edge(2, (20.0, 0.0), (20.0, 20.0)),
                    edge(3, (20.0, 20.0), (0.0, 20.0)),
                    edge(4, (0.0, 20.0), (0.0, 0.0)),
                ],
            },
            ProfileWire {
                key: 8,
                // The circle runs through two half edges.
                edges: vec![
                    edge(5, (13.0, 10.0), (7.0, 10.0)),
                    edge(5, (7.0, 10.0), (13.0, 10.0)),
                ],
            },
        ],
    }
}

fn index<'a>(
    bodies: &'a [ProfileBody],
    links: &'a [PmDcKernelEntityLink],
    entities: &'a [SketchEntity],
) -> ProfileIndex<'a> {
    let mut index = ProfileIndex::new(bodies, links, &[], &[], entities);
    index.curves = (1..=5)
        .map(|id| {
            (
                ("segment", SKETCH_ORDINAL, id),
                Some(format!("inventor:pmdc:sketch-entity#segment-{}", 10 + id)),
            )
        })
        .collect::<HashMap<_, _>>();
    index
}

#[test]
fn a_loop_inside_another_selected_loop_is_a_hole_of_its_region() {
    let (sketch, entities) = square_with_hole();
    let bodies = [profile_body((0.0, 0.0))];
    let links = [kernel_link(20, 7), kernel_link(21, 8)];
    let index = index(&bodies, &links, &entities);
    let profile = index
        .regions("segment", SKETCH_ORDINAL, &sketch, &[20, 21])
        .expect("resolved profile");
    let PlanarProfileRef::SketchRegions { regions, .. } = profile else {
        panic!("profile regions");
    };
    assert_eq!(
        regions.as_slice(),
        [SketchProfileRegion::loops(0, vec![1]).expect("region")]
    );
    // The circle alone selects the disc.
    let disc = index
        .regions("segment", SKETCH_ORDINAL, &sketch, &[21])
        .expect("resolved disc");
    let PlanarProfileRef::SketchRegions { regions, .. } = disc else {
        panic!("profile regions");
    };
    assert_eq!(
        regions.as_slice(),
        [SketchProfileRegion::loops(1, Vec::new()).expect("region")]
    );
}

#[test]
fn a_wire_edge_shorter_than_its_sketch_curve_leaves_the_selection_unresolved() {
    let (sketch, entities) = square_with_hole();
    let bodies = [profile_body((5.0, 0.0))];
    let links = [kernel_link(20, 7)];
    let index = index(&bodies, &links, &entities);
    assert!(index
        .regions("segment", SKETCH_ORDINAL, &sketch, &[20])
        .is_none());
}
