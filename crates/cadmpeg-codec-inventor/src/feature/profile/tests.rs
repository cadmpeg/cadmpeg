use std::collections::HashMap;

use cadmpeg_ir::attributes::AttributeValue;
use cadmpeg_ir::features::{PlanarProfileRef, SketchProfileBoundaryUse, SketchProfileRegion};
use cadmpeg_ir::geometry::DirectedParameterRange;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchPlacement, SketchProfiles,
};

use super::{
    chord_turn, edge_name, encloses, EdgeName, LoopCurve, ProfileBody, ProfileEdge, ProfileIndex,
    ProfileWire,
};
use crate::feature::{
    PmDcEntityStyleLinkPayload, PmDcKernelEntityLink, PmDcKernelEntityLinkPayload,
    PmDcLinkedHeader, ENTITY_STYLE_LINK_TYPE, KERNEL_ENTITY_LINK_TYPE,
};
use crate::pmdc::PmDcReference;
use crate::record_identity::{Located, RecordTypeId};
use crate::sketch::PmDcSketchEntity;

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
        edge_name(super::NAMING_ATTRIBUTE, &naming_values(38, 14, 2)),
        Some(EdgeName {
            naming_type: 38,
            associative_id: 14,
        })
    );
    // A vertex name and a truncated attribute name no sketch curve.
    assert_eq!(
        edge_name(super::NAMING_ATTRIBUTE, &naming_values(38, 14, 1)),
        None
    );
    assert_eq!(
        edge_name(super::NAMING_ATTRIBUTE, &naming_values(38, 14, 2)[..9]),
        None
    );
}

#[test]
fn custom_edge_name_requires_its_family_version_and_exact_integer_layout() {
    let mut values = naming_values(44, 7, 2);
    values.splice(
        5..5,
        [
            AttributeValue::String("INV_NMX_GEN_TAG".into()),
            AttributeValue::Integer(1),
        ],
    );
    assert_eq!(
        edge_name(super::CUSTOM_NAMING_ATTRIBUTE, &values),
        Some(EdgeName {
            naming_type: 44,
            associative_id: 7,
        })
    );
    assert_eq!(edge_name(super::NAMING_ATTRIBUTE, &values), None);
    assert_eq!(edge_name("other-attrib", &values), None);
    assert_eq!(
        edge_name(super::CUSTOM_NAMING_ATTRIBUTE, &values[..11]),
        None
    );
    values.push(AttributeValue::Integer(0));
    assert_eq!(edge_name(super::CUSTOM_NAMING_ATTRIBUTE, &values), None);
    values.pop();
    values[11] = AttributeValue::Integer(1);
    assert_eq!(edge_name(super::CUSTOM_NAMING_ATTRIBUTE, &values), None);
    values[11] = AttributeValue::Integer(2);
    values[6] = AttributeValue::Integer(2);
    assert_eq!(edge_name(super::CUSTOM_NAMING_ATTRIBUTE, &values), None);
    values[6] = AttributeValue::Integer(1);
    values[5] = AttributeValue::String("other_family".into());
    assert_eq!(edge_name(super::CUSTOM_NAMING_ATTRIBUTE, &values), None);
    assert_eq!(
        edge_name(super::CUSTOM_NAMING_ATTRIBUTE, &naming_values(44, 7, 2)),
        None
    );
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
        middle: None,
    }
}

/// An edge whose carrier passes through `middle` halfway along it.
fn edge_through(
    associative_id: i64,
    start: (f64, f64),
    middle: (f64, f64),
    end: (f64, f64),
) -> ProfileEdge {
    ProfileEdge {
        middle: Some(Point3::new(middle.0, middle.1, 0.0)),
        ..edge(associative_id, start, end)
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
    index.curves = (1..=8)
        .map(|id| {
            (
                ("segment", SKETCH_ORDINAL, id),
                Some(format!("inventor:design:sketch-entity#segment-{}", 10 + id)),
            )
        })
        .collect::<HashMap<_, _>>();
    index
}

#[test]
fn a_profile_face_loop_uses_its_body_key_and_custom_edge_names() {
    use cadmpeg_asm::brep::{records::BodyNativeKey, AsmBrep};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::attributes::{AttributeTarget, SourceAttribute};
    use cadmpeg_ir::topology::{
        Coedge, Edge, EdgeCarrier, Face, FaceLoops, Loop, LoopBoundary, LoopRing, Region, Sense,
        Shell, Vertex,
    };

    let id = |index| format!("inventor:brep:entity#{index}");
    let mut brep = AsmBrep::default();
    brep.regions.push(Region {
        id: id(2).try_into().expect("synthetic identity"),
        body: id(1).try_into().expect("synthetic identity"),
        shells: vec![id(3).try_into().expect("synthetic identity")],
    });
    brep.shells.push(
        Shell::with_faces(
            id(3).try_into().expect("synthetic identity"),
            id(2).try_into().expect("synthetic identity"),
            vec![id(4).try_into().expect("synthetic identity")],
        )
        .expect("synthetic profile fixture"),
    );
    brep.faces.push(Face {
        id: id(4).try_into().expect("synthetic identity"),
        shell: id(3).try_into().expect("synthetic identity"),
        surface: id(6).try_into().expect("synthetic identity"),
        sense: Sense::Forward,
        loops: FaceLoops::Classified {
            outer: id(5).try_into().expect("synthetic identity"),
            inner: vec![],
        },
        name: None,
        color: None,
        tolerance: None,
    });
    brep.loops.push(Loop {
        id: id(5).try_into().expect("synthetic identity"),
        face: id(4).try_into().expect("synthetic identity"),
        boundary: LoopBoundary::Ring(
            LoopRing::new(
                (21..25)
                    .map(|i| id(i).try_into().expect("synthetic identity"))
                    .collect(),
                vec![],
            )
            .expect("synthetic profile fixture"),
        ),
    });
    let points = [(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)];
    for (i, (x, y)) in (0..4).zip(points) {
        brep.points.push(
            serde_json::from_value(serde_json::json!({
                "id": id(41+i), "position": {"x": x, "y": y, "z": 0.0}
            }))
            .expect("synthetic profile fixture"),
        );
        brep.vertices.push(Vertex {
            id: id(31 + i).try_into().expect("synthetic identity"),
            point: id(41 + i).try_into().expect("synthetic identity"),
            tolerance: None,
        });
        brep.edges.push(Edge {
            id: id(11 + i).try_into().expect("synthetic identity"),
            carrier: EdgeCarrier::unbounded(None),
            start: id(31 + i).try_into().expect("synthetic identity"),
            end: id(31 + (i + 1) % 4).try_into().expect("synthetic identity"),
            tolerance: None,
        });
        brep.coedges.push(Coedge {
            id: id(21 + i).try_into().expect("synthetic identity"),
            owner_loop: id(5).try_into().expect("synthetic identity"),
            edge: id(11 + i).try_into().expect("synthetic identity"),
            radial_next: id(21 + i).try_into().expect("synthetic identity"),
            sense: Sense::Forward,
            pcurves: vec![],
            use_curve: None,
        });
        let mut values = naming_values(i64::from(NAMING_TYPE), i64::from(i + 1), 2);
        values.splice(
            5..5,
            [
                AttributeValue::String("INV_NMX_GEN_TAG".into()),
                AttributeValue::Integer(1),
            ],
        );
        brep.attributes.push(SourceAttribute {
            id: format!("inventor:brep:attribute#{}", 51 + i)
                .try_into()
                .expect("synthetic profile fixture"),
            target: AttributeTarget::Edge(id(11 + i).try_into().expect("synthetic identity")),
            name: super::CUSTOM_NAMING_ATTRIBUTE.into(),
            values,
        });
    }
    brep.attributes.push(SourceAttribute {
        id: "inventor:brep:attribute#60"
            .try_into()
            .expect("synthetic identity"),
        target: AttributeTarget::Edge(id(11).try_into().expect("synthetic identity")),
        name: super::CUSTOM_NAMING_ATTRIBUTE.into(),
        values: vec![AttributeValue::Integer(0); 5]
            .into_iter()
            .chain([AttributeValue::String("other_family".into())])
            .collect(),
    });
    brep.body_native_keys.push(BodyNativeKey {
        source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
            cadmpeg_asm::asm_format!("inventor"),
        ),
        record_index: 1,
        body: id(1).try_into().expect("synthetic identity"),
        body_ordinal: 0,
        source_brep: None,
        asm_body_key: Some(7),
    });
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("synthetic profile fixture");
    let wires = super::profile_wires(&ctx, &brep).expect("synthetic profile fixture");
    assert_eq!(wires, vec![profile_body((0.0, 0.0)).wires.remove(0)]);
    brep.coedges.pop();
    let wires = super::profile_wires(&ctx, &brep).expect("synthetic profile fixture");
    assert_eq!(wires.len(), 1);
    assert_eq!(wires[0].key, 7);
    assert!(wires[0].edges.is_empty());
}

#[test]
fn custom_edge_names_restore_a_selected_loop_that_the_sketch_table_lacks() {
    let (mut sketch, entities) = square_with_hole();
    sketch.profiles = SketchProfiles::default();
    let mut body = profile_body((0.0, 0.0));
    for (offset, edge) in body.wires[0].edges.iter_mut().enumerate() {
        let associative_id = i64::try_from(offset + 1).expect("fixture id");
        let mut values = naming_values(i64::from(NAMING_TYPE), associative_id, 2);
        values.splice(
            5..5,
            [
                AttributeValue::String("INV_NMX_GEN_TAG".into()),
                AttributeValue::Integer(1),
            ],
        );
        edge.name = edge_name(super::CUSTOM_NAMING_ATTRIBUTE, &values);
    }
    let bodies = [body];
    let links = [kernel_link(20, 7)];
    let index = index(&bodies, &links, &entities);
    let profile = index
        .regions("segment", SKETCH_ORDINAL, &sketch, &[20])
        .expect("selected square resolves");
    let PlanarProfileRef::SketchRegions { regions, .. } = profile else {
        panic!("profile regions");
    };
    assert_eq!(
        regions.as_slice(),
        [SketchProfileRegion::loops(0, vec![]).expect("region")]
    );
    let added = index.into_added_profiles();
    let loops = &added[sketch.id.as_str()];
    assert_eq!(loops.len(), 1);
    assert_eq!(loops[0].len(), 4);
    for (use_, entity) in loops[0].iter().zip(&entities[..4]) {
        assert_eq!(&use_.entity, entity.id());
        assert!(!use_.reversed);
    }
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

/// A raw sketch entity record of `kind` at `ordinal`, in the sketch or block
/// definition at one-based `sketch`.
fn raw_entity(ordinal: u32, sketch: u32, kind: &serde_json::Value) -> PmDcSketchEntity {
    serde_json::from_value(serde_json::json!({
        "id": format!("inventor:pmdc:sketch-entity#segment-{ordinal}"),
        "type_id": "3bdf52ced011d0d20008ccbc0663dc09",
        "segment_token": "segment",
        "record_ordinal": ordinal,
        "save_version_major": 22,
        "header": {
            "header_value": 0,
            "header_id": 0,
            "flags": 0,
            "source_index": 0,
            "next": {"index": 0, "qualified": false},
            "context": {"index": 0, "qualified": false},
        },
        "entity_flags": 0,
        "sketch": {"index": sketch, "qualified": true},
        "kind": kind,
    }))
    .expect("raw sketch entity")
}

#[test]
fn a_curve_that_a_block_instance_places_resolves_through_the_instance_identifiers() {
    let (mut sketch, mut entities) = square_with_hole();
    // Block instance 16 places definition record 40, whose circle 41 has
    // radius 0.3 cm about the definition origin, at (1, 1) cm.
    let instance = raw_entity(
        16,
        SKETCH_ORDINAL + 1,
        &serde_json::json!({
            "form": "block_instance",
            "definition": {"index": 41, "qualified": true},
            "group": {"index": 0, "qualified": false},
            "masks": [0x0111, 0x00ca],
            "placement": [[1.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 0.0, 1.0]],
            "value": 1,
        }),
    );
    let circle = raw_entity(
        41,
        41,
        &serde_json::json!({
            "form": "circle",
            "points": {
                "marker": 2,
                "metadata": {"values": [2, 0], "width": "u32"},
                "references": [
                    {"index": 0, "qualified": false},
                    {"index": 0, "qualified": false},
                ],
            },
            "auxiliary": [],
            "center": {"index": 41, "qualified": true},
            "radius": 0.3,
            "state": 1,
        }),
    );
    let placed = SketchEntity::new(
        crate::sketch::block_curve_id(&instance, &circle).expect("placed curve id"),
        sketch_id(),
        entities[4].geometry.clone(),
    )
    .with_native_ref(Some(circle.id()));
    entities[4] = placed.clone();
    sketch.profiles = SketchProfiles::try_from(vec![
        entities[..4]
            .iter()
            .map(|entity| SketchEntityUse {
                entity: entity.id().clone(),
                reversed: false,
            })
            .collect(),
        vec![SketchEntityUse {
            entity: placed.id().clone(),
            reversed: false,
        }],
    ])
    .expect("profiles");
    // The instance's associative identifier is 10, so its first placed
    // curve, the circle, is 12.
    let instance_link = Located::new(
        PmDcEntityStyleLinkPayload {
            save_version_major: 22,
            header: PmDcLinkedHeader {
                header_value: 0,
                header_id: 0,
                values: [0; 2],
                flags_extension: None,
                owner: PmDcReference::new(17, true).expect("owner reference"),
                parent: PmDcReference::new(0, false).expect("null reference"),
                next: PmDcReference::new(0, false).expect("null reference"),
            },
            value: 0,
            associative_id: 10,
            entity_type: 0x0800_0000,
        },
        RecordTypeId::from_bytes(ENTITY_STYLE_LINK_TYPE),
        cadmpeg_ir::identity_key!("segment"),
        60,
    );
    let mut body = profile_body((0.0, 0.0));
    for edge in &mut body.wires[1].edges {
        edge.name = Some(EdgeName {
            naming_type: i64::from(NAMING_TYPE),
            associative_id: 12,
        });
    }
    let bodies = [body];
    let links = [kernel_link(20, 7), kernel_link(21, 8)];
    let raw = [instance, circle];
    let style_links = [instance_link];
    let mut index = ProfileIndex::new(&bodies, &links, &style_links, &raw, &entities);
    for id in 1..=4 {
        index.curves.insert(
            ("segment", SKETCH_ORDINAL, id),
            Some(format!("inventor:design:sketch-entity#segment-{}", 10 + id)),
        );
    }
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
}

#[test]
fn a_wire_that_does_not_close_leaves_the_selection_unresolved() {
    let (sketch, entities) = square_with_hole();
    let bodies = [profile_body((5.0, 0.0))];
    let links = [kernel_link(20, 7)];
    let index = index(&bodies, &links, &entities);
    assert!(index
        .regions("segment", SKETCH_ORDINAL, &sketch, &[20])
        .is_none());
}

/// The winding number of a closed loop about `point`, from a dense polyline
/// through its curves.
fn polyline_winding(curves: &[LoopCurve], point: Point2) -> f64 {
    let mut turns = 0.0;
    for curve in curves {
        let samples = match *curve {
            LoopCurve::Arc {
                center,
                radius,
                start_angle,
                sweep,
            } => (0..=4000)
                .map(|step| {
                    let angle = sweep.mul_add(f64::from(step) / 4000.0, start_angle);
                    Point2::new(
                        radius.mul_add(angle.cos(), center.u),
                        radius.mul_add(angle.sin(), center.v),
                    )
                })
                .collect::<Vec<_>>(),
            _ => {
                let (start, end) = curve.ends();
                vec![start, end]
            }
        };
        for pair in samples.windows(2) {
            turns += chord_turn(pair[0], pair[1], point);
        }
    }
    turns / std::f64::consts::TAU
}

#[test]
fn an_arc_turns_by_its_chord_and_the_segment_it_bounds() {
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};
    let arc = |center: (f64, f64), radius: f64, start_angle: f64, sweep: f64| LoopCurve::Arc {
        center: Point2::new(center.0, center.1),
        radius,
        start_angle,
        sweep,
    };
    let radius = 0.5_f64.sqrt();
    // From (1, 0) over the top to (0, 0), on the circle about (0.5, -0.5).
    let upper = arc((0.5, -0.5), radius, FRAC_PI_4, FRAC_PI_2);
    let loops = [
        // A lens: back along the circle about (0.5, 0.5).
        vec![upper, arc((0.5, 0.5), radius, -3.0 * FRAC_PI_4, FRAC_PI_2)],
        // A crescent: back the long way around that circle.
        vec![upper, arc((0.5, 0.5), radius, -3.0 * FRAC_PI_4, -1.5 * PI)],
    ];
    for curves in &loops {
        for i in 0..=40 {
            for j in 0..=40 {
                let point = Point2::new(
                    f64::from(i).mul_add(0.05, -0.5),
                    f64::from(j).mul_add(0.05, -1.0),
                );
                let near_curve = [(0.5, -0.5), (0.5, 0.5)].iter().any(|center| {
                    ((point.u - center.0).hypot(point.v - center.1) - radius).abs() < 1.0e-2
                });
                if near_curve {
                    continue;
                }
                assert_eq!(
                    encloses(curves, point),
                    Some(polyline_winding(curves, point).round() != 0.0),
                    "{point:?}"
                );
            }
        }
    }
    // A point inside the circle of an arc but on the far side of its chord.
    let crescent = [
        arc((0.5, 0.726), 0.882, -2.174, 1.206),
        arc((0.5, -0.417), 0.651, 0.695, -4.531),
    ];
    assert_eq!(encloses(&crescent, Point2::new(0.898, 0.053)), Some(false));
}

#[test]
fn a_ring_along_parts_of_curves_is_a_trimmed_region() {
    // A divider from (10, 0) to (10, 20) splits the square; the selection is
    // its left half.
    let (sketch, mut entities) = square_with_hole();
    entities.push(entity(
        16,
        SketchGeometryDefinition::Line {
            start: Point2::new(10.0, 0.0),
            end: Point2::new(10.0, 20.0),
        },
    ));
    let bodies = [ProfileBody {
        segment_token: cadmpeg_ir::identity_key!("segment"),
        record_ordinal: BODY_ORDINAL,
        wires: vec![ProfileWire {
            key: 9,
            edges: vec![
                edge(1, (0.0, 0.0), (10.0, 0.0)),
                edge(6, (10.0, 0.0), (10.0, 20.0)),
                edge(3, (10.0, 20.0), (0.0, 20.0)),
                edge(4, (0.0, 20.0), (0.0, 0.0)),
            ],
        }],
    }];
    let links = [kernel_link(20, 9)];
    let index = index(&bodies, &links, &entities);
    let profile = index
        .regions("segment", SKETCH_ORDINAL, &sketch, &[20])
        .expect("resolved profile");
    let PlanarProfileRef::SketchRegions { regions, .. } = profile else {
        panic!("profile regions");
    };
    let boundary_use = |ordinal: u32, range: [f64; 2]| SketchProfileBoundaryUse {
        entity: SketchEntityId::mint(format!("inventor:design:sketch-entity#segment-{ordinal}"))
            .expect("entity id"),
        parameter_range: DirectedParameterRange::new(range).expect("range"),
        reversed: false,
    };
    assert_eq!(
        regions.as_slice(),
        [SketchProfileRegion::trimmed(
            vec![
                boundary_use(11, [0.0, 0.5]),
                boundary_use(16, [0.0, 1.0]),
                boundary_use(13, [0.5, 1.0]),
                boundary_use(14, [0.0, 1.0]),
            ],
            Vec::new(),
        )
        .expect("region")]
    );
}

fn boundary_use(ordinal: u32, range: [f64; 2], reversed: bool) -> SketchProfileBoundaryUse {
    SketchProfileBoundaryUse {
        entity: SketchEntityId::mint(format!("inventor:design:sketch-entity#segment-{ordinal}"))
            .expect("entity id"),
        parameter_range: DirectedParameterRange::new(range).expect("range"),
        reversed,
    }
}

fn trimmed_boundary(
    sketch: &Sketch,
    entities: &[SketchEntity],
    edges: Vec<ProfileEdge>,
) -> Vec<SketchProfileBoundaryUse> {
    let bodies = [ProfileBody {
        segment_token: cadmpeg_ir::identity_key!("segment"),
        record_ordinal: BODY_ORDINAL,
        wires: vec![ProfileWire { key: 9, edges }],
    }];
    let links = [kernel_link(20, 9)];
    let index = index(&bodies, &links, entities);
    let profile = index
        .regions("segment", SKETCH_ORDINAL, sketch, &[20])
        .expect("resolved profile");
    let PlanarProfileRef::SketchRegions { regions, .. } = profile else {
        panic!("profile regions");
    };
    let [region] = regions.as_slice() else {
        panic!("one region");
    };
    let SketchProfileRegion::Trimmed {
        outer_boundary,
        hole_boundaries,
    } = region
    else {
        panic!("trimmed region");
    };
    assert!(hole_boundaries.is_empty());
    outer_boundary.to_vec()
}

#[test]
fn an_arc_piece_entered_at_its_start_angle_runs_forward() {
    use std::f64::consts::FRAC_PI_2;
    // The right half of a circle about (10, 10) from (10, 0) to (10, 20),
    // closed below the middle by a radius along v = 10 and the lower half of
    // the diameter. The wire enters the arc a hair below its start angle.
    let (sketch, mut entities) = square_with_hole();
    entities.extend([
        entity(
            16,
            SketchGeometryDefinition::Arc {
                center: Point2::new(10.0, 10.0),
                radius: Length::new(10.0).expect("radius"),
                start_angle: Angle::new(-FRAC_PI_2).expect("angle"),
                end_angle: Angle::new(FRAC_PI_2).expect("angle"),
            },
        ),
        entity(
            17,
            SketchGeometryDefinition::Line {
                start: Point2::new(10.0, 10.0),
                end: Point2::new(20.0, 10.0),
            },
        ),
        entity(
            18,
            SketchGeometryDefinition::Line {
                start: Point2::new(10.0, 0.0),
                end: Point2::new(10.0, 20.0),
            },
        ),
    ]);
    let boundary = trimmed_boundary(
        &sketch,
        &entities,
        vec![
            edge(6, (10.0 - 1.0e-12, 0.0), (20.0, 10.0)),
            edge(7, (20.0, 10.0), (10.0, 10.0)),
            edge(8, (10.0, 10.0), (10.0, 0.0)),
        ],
    );
    assert_eq!(
        boundary,
        [
            boundary_use(16, [-FRAC_PI_2, 0.0], false),
            boundary_use(17, [0.0, 1.0], true),
            boundary_use(18, [0.0, 0.5], true),
        ]
    );
}

#[test]
fn a_ring_along_part_of_a_circle_runs_through_the_edge_middle() {
    use std::f64::consts::{FRAC_PI_2, PI};
    // The right half of the 3 mm circle, closed by its vertical diameter.
    let (sketch, mut entities) = square_with_hole();
    entities.push(entity(
        16,
        SketchGeometryDefinition::Line {
            start: Point2::new(10.0, 7.0),
            end: Point2::new(10.0, 13.0),
        },
    ));
    let boundary = trimmed_boundary(
        &sketch,
        &entities,
        vec![
            edge_through(5, (10.0, 7.0), (13.0, 10.0), (10.0, 13.0)),
            edge(6, (10.0, 13.0), (10.0, 7.0)),
        ],
    );
    assert_eq!(boundary.len(), 2);
    let range = boundary[0].parameter_range.endpoints();
    assert!((range[0] - 3.0 * FRAC_PI_2).abs() < 1.0e-12, "{range:?}");
    assert!((range[1] - range[0] - PI).abs() < 1.0e-12, "{range:?}");
    assert!(!boundary[0].reversed);
    assert_eq!(boundary[1], boundary_use(16, [0.0, 1.0], true));
}
