use std::collections::BTreeSet;

use cadmpeg_ir::sketches::SketchEntityId;

use super::{join, polygon_faces, PathFace, Record, SideGenerators, SurfaceClasses};
use crate::records::{
    FeatureInputClass, FeatureInputComponentPathEntry, FeatureInputEdgeSelection,
    FeatureInputGeneratedSurfaceIdentity, FeatureInputLane, LineSides,
};

fn entity(name: &str) -> SketchEntityId {
    SketchEntityId::mint(format!("synthetic:test:id#{name}")).expect("identity grammar")
}

/// A record with the words `state` and `next`, and the line from `a` to `b`.
fn record(state: u32, next: u32, line: Option<([f64; 2], [f64; 2])>) -> Record {
    Record {
        sides: LineSides {
            state,
            next_side: Some(next),
        },
        read: line.map(|(a, b)| (a, b, entity("line"))),
    }
}

fn ids(values: &[u32]) -> BTreeSet<u32> {
    values.iter().copied().collect()
}

/// The four sides of a square, in contour order.
fn square() -> [([f64; 2], [f64; 2]); 4] {
    [
        ([0.0, 0.0], [1.0, 0.0]),
        ([1.0, 0.0], [1.0, 1.0]),
        ([1.0, 1.0], [0.0, 1.0]),
        ([0.0, 1.0], [0.0, 0.0]),
    ]
}

#[test]
fn a_record_names_the_face_of_the_next_line_and_the_first_line_takes_the_remaining_one() {
    // Records 0 to 2 store the identifiers of lines 1 to 3; identifier 2 is
    // the one no record names.
    let records = [
        record(0, 3, None),
        record(0, 1, None),
        record(0, 4, None),
        record(0, 0, None),
    ];
    assert_eq!(
        join(&records, &ids(&[1, 2, 3, 4])),
        [Some(2), Some(3), Some(1), Some(4)]
    );
}

#[test]
fn identifiers_may_start_above_one() {
    let records = [
        record(0, 9, None),
        record(0, 7, None),
        record(0, 8, None),
        record(0, 0, None),
    ];
    assert_eq!(
        join(&records, &ids(&[6, 7, 8, 9])),
        [Some(6), Some(9), Some(7), Some(8)]
    );
}

#[test]
fn a_record_with_state_states_no_face_for_the_next_line_and_leaves_the_first_ambiguous() {
    let records = [
        record(0, 3, None),
        record(2, 1, None),
        record(0, 4, None),
        record(0, 0, None),
    ];
    // Lines 0 and 2 are unnamed and two identifiers remain.
    assert_eq!(
        join(&records, &ids(&[1, 2, 3, 4])),
        [None, Some(3), None, Some(4)]
    );
}

#[test]
fn an_unnamed_identifier_outside_the_sides_voids_the_join() {
    let records = [
        record(0, 3, None),
        record(0, 5, None),
        record(0, 4, None),
        record(0, 0, None),
    ];
    assert_eq!(join(&records, &ids(&[1, 2, 3, 4])), [None; 4]);
}

#[test]
fn a_repeated_identifier_voids_the_join() {
    let records = [
        record(0, 3, None),
        record(0, 3, None),
        record(0, 4, None),
        record(0, 0, None),
    ];
    assert_eq!(join(&records, &ids(&[1, 2, 3, 4])), [None; 4]);
}

#[test]
fn a_last_record_that_names_a_face_voids_the_join() {
    let records = [
        record(0, 3, None),
        record(0, 1, None),
        record(0, 4, None),
        record(0, 2, None),
    ];
    assert_eq!(join(&records, &ids(&[1, 2, 3, 4])), [None; 4]);
}

#[test]
fn a_contour_that_closes_is_joined() {
    let lines = square();
    let records = [
        record(0, 3, Some(lines[0])),
        record(0, 1, Some(lines[1])),
        record(0, 4, Some(lines[2])),
        record(0, 0, Some(lines[3])),
    ];
    assert_eq!(
        polygon_faces(&records, &ids(&[1, 2, 3, 4])),
        [Some(2), Some(3), Some(1), Some(4)]
    );
}

#[test]
fn records_whose_lines_share_no_corner_state_nothing() {
    let lines = square();
    // Lines 0 and 2 follow each other: they share no corner.
    let records = [
        record(0, 3, Some(lines[0])),
        record(0, 1, Some(lines[2])),
        record(0, 4, Some(lines[1])),
        record(0, 0, Some(lines[3])),
    ];
    assert_eq!(polygon_faces(&records, &ids(&[1, 2, 3, 4])), [None; 4]);
}

#[test]
fn a_line_whose_geometry_is_not_read_does_not_break_the_contour() {
    let lines = square();
    let records = [
        record(0, 3, Some(lines[0])),
        record(0, 1, None),
        record(0, 4, Some(lines[2])),
        record(0, 0, Some(lines[3])),
    ];
    let faces = polygon_faces(&records, &ids(&[1, 2, 3, 4]));
    assert_eq!(faces, [Some(2), Some(3), Some(1), Some(4)]);
    // Only a record with a read line carries a generator.
    assert!(records[1].read.is_none());
}

#[test]
fn fewer_than_three_records_are_no_polygon() {
    let records = [record(0, 1, None), record(0, 0, None)];
    assert_eq!(polygon_faces(&records, &ids(&[1, 2])), [None; 2]);
}

fn identity(
    offset: u64,
    instance: Option<u16>,
    local: u32,
) -> FeatureInputGeneratedSurfaceIdentity {
    FeatureInputGeneratedSurfaceIdentity {
        id: format!("identity-{offset}"),
        parent: "lane".into(),
        ordinal: 0,
        offset,
        type_prefix: [0; 4].into(),
        feature_source_id: 45.try_into().expect("source id"),
        local_identity: local,
        components: vec![FeatureInputComponentPathEntry {
            instance,
            type_signature: [0; 12].into(),
            local_id: Some(local),
        }],
    }
}

fn class(offset: u64, name: &str) -> FeatureInputClass {
    FeatureInputClass {
        id: format!("class-{offset}"),
        parent: "lane".into(),
        ordinal: 0,
        offset,
        name: name.into(),
    }
}

/// An end-face class declared at 100 and a side-face class at 200, each with
/// its first identity inline, then identities that carry the classes' cells.
fn surface_lane(side: u16, end: u16) -> FeatureInputLane {
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new().into(),
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
    };
    // A declaration is 6 bytes, its name, then 2 bytes before the identity.
    lane.classes = vec![
        class(100, "moEndFaceSurfIdRep_c"),
        class(200, "moFromSktEntSurfIdRep_c"),
    ];
    lane.generated_surface_identities = vec![
        identity(128, None, 0),
        identity(231, None, 6),
        identity(300, Some(side), 4),
        identity(320, Some(side), 2),
        identity(400, Some(end), 1),
    ];
    lane
}

#[test]
fn the_declared_classes_take_the_cells_in_ascending_order() {
    let lane = surface_lane(0x814e, 0x814b);
    let classes = SurfaceClasses::of(&lane).expect("classes");
    assert_eq!((classes.side, classes.end), (0x814e, 0x814b));
    assert_eq!(classes.face(Some(0x814e), Some(4)), Some(PathFace::Side(4)));
    assert_eq!(classes.face(Some(0x814b), Some(1)), Some(PathFace::End));
    assert_eq!(classes.face(Some(0x814b), Some(2)), None);
    assert_eq!(classes.face(Some(0x814b), None), None);
    assert_eq!(classes.face(Some(0x8150), Some(1)), None);
    assert_eq!(classes.face(None, Some(1)), None);
}

#[test]
fn the_first_inline_identity_of_the_side_class_is_a_side_face() {
    let lane = surface_lane(0x814e, 0x814b);
    let classes = SurfaceClasses::of(&lane).expect("classes");
    let sides = lane
        .generated_surface_identities
        .iter()
        .filter(|identity| classes.is_side(identity))
        .map(|identity| identity.local_identity)
        .collect::<BTreeSet<_>>();
    assert_eq!(sides, ids(&[2, 4, 6]));
}

#[test]
fn a_cell_with_no_declared_class_states_no_classes() {
    let mut lane = surface_lane(0x814e, 0x814b);
    lane.generated_surface_identities
        .push(identity(500, Some(0x8150), 3));
    assert_eq!(SurfaceClasses::of(&lane), None);
}

#[test]
fn a_cell_first_used_before_its_declaration_states_no_classes() {
    let mut lane = surface_lane(0x814e, 0x814b);
    lane.generated_surface_identities[2].offset = 150;
    assert_eq!(SurfaceClasses::of(&lane), None);
}

fn path(entries: [(u16, u32); 4]) -> FeatureInputEdgeSelection {
    FeatureInputEdgeSelection {
        id: "edge".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        object_name_ref: "name".into(),
        feature_ref: "fillet".into(),
        local_edge_ids: entries.iter().map(|(_, local)| *local).collect(),
        components: entries
            .iter()
            .map(|(instance, local)| FeatureInputComponentPathEntry {
                instance: Some(*instance),
                type_signature: [0; 12].into(),
                local_id: Some(*local),
            })
            .collect(),
        references: Vec::new(),
        producer_feature_refs: Vec::new(),
        terminal_feature_ref: Some("extrude".into()),
        mark_reference: false,
    }
}

#[test]
fn a_path_settles_when_each_side_face_it_names_has_a_generator() {
    let classes = SurfaceClasses::for_test(0x814e, 0x814b);
    let generators = SideGenerators::default()
        .with_face(45, 4, entity("four"))
        .with_face(45, 2, entity("two"));
    // The edge between sides 4 and 2, ending on the end faces 1 and 0.
    let vertical = path([(0x814e, 4), (0x814e, 2), (0x814b, 1), (0x814b, 0)]);
    assert!(generators.settle(&vertical, &classes, 45));
    assert!(!generators.settle(&vertical, &classes, 46));
    // Side 3 has no generator.
    let other = path([(0x814e, 4), (0x814e, 3), (0x814b, 1), (0x814b, 0)]);
    assert!(!generators.settle(&other, &classes, 45));
}

#[test]
fn a_path_entry_of_no_known_class_leaves_the_path_unsettled() {
    let classes = SurfaceClasses::for_test(0x814e, 0x814b);
    let generators = SideGenerators::default().with_face(45, 4, entity("four"));
    let unknown = path([(0x814e, 4), (0x8150, 2), (0x814b, 1), (0x814b, 0)]);
    assert!(!generators.settle(&unknown, &classes, 45));
}

#[test]
fn a_face_with_two_generators_has_none() {
    let generators = SideGenerators::default()
        .with_face(45, 4, entity("a"))
        .with_face(45, 4, entity("b"));
    assert_eq!(generators.generator(45, 4), None);
    assert!(generators.into_stated(|_| true).is_empty());
}

/// A 92-byte record at 0 with the words `state` at +80 and `next` at +84,
/// followed by the next sketch marker.
fn record_bytes(state: u32, next: u32) -> Vec<u8> {
    let mut bytes = vec![0u8; 97];
    bytes[..5].copy_from_slice(&[0xff, 0xff, 0x1f, 0x00, 0x03]);
    bytes[80..84].copy_from_slice(&state.to_le_bytes());
    bytes[84..88].copy_from_slice(&next.to_le_bytes());
    bytes[92..97].copy_from_slice(&[0xff, 0xff, 0x1f, 0x00, 0x03]);
    bytes
}

#[test]
fn the_side_words_are_the_u32_at_80_and_84_of_a_record_that_ends_at_the_next_marker() {
    let bytes = record_bytes(0x20000, 7);
    assert_eq!(
        crate::resolved_features::markers::marker_line_sides(&bytes, 0),
        Some(LineSides {
            state: 0x20000,
            next_side: Some(7)
        })
    );
    // No marker 92 bytes on: not a 92-byte record.
    let mut open = bytes.clone();
    open[92] = 0;
    assert_eq!(
        crate::resolved_features::markers::marker_line_sides(&open, 0),
        None
    );
}

#[test]
fn a_direct_form_record_states_no_side_word() {
    // Value 1 at +84 with a nonzero identity at +88 is the direct-form
    // selector of the profile line, not a side identifier.
    let mut bytes = record_bytes(0, 1);
    bytes[88..92].copy_from_slice(&5u32.to_le_bytes());
    let sides = crate::resolved_features::markers::marker_line_sides(&bytes, 0).unwrap();
    assert_eq!(
        sides,
        LineSides {
            state: 0,
            next_side: None
        }
    );
    // Without the identity the word is a side identifier.
    for identity in [0, u32::MAX] {
        bytes[88..92].copy_from_slice(&identity.to_le_bytes());
        let sides = crate::resolved_features::markers::marker_line_sides(&bytes, 0).unwrap();
        assert_eq!(sides.next_side, Some(1));
    }
}

fn direct_record() -> Record {
    Record {
        sides: LineSides {
            state: 0,
            next_side: None,
        },
        read: None,
    }
}

#[test]
fn a_direct_form_record_in_a_polygon_is_not_read_as_a_side_identifier() {
    // Side identifier 1 exists and no other record names it. The direct-form
    // record in the middle names no face for its successor, so the join
    // leaves two lines unnamed and states nothing for the first.
    let records = [
        record(0, 3, None),
        direct_record(),
        record(0, 4, None),
        record(0, 0, None),
    ];
    assert_eq!(
        join(&records, &ids(&[1, 2, 3, 4])),
        [None, Some(3), None, Some(4)]
    );
    // As the last record it does not void the join.
    let records = [
        record(0, 3, None),
        record(0, 2, None),
        record(0, 4, None),
        direct_record(),
    ];
    assert_eq!(
        join(&records, &ids(&[1, 2, 3, 4])),
        [Some(1), Some(3), Some(2), Some(4)]
    );
}
