// SPDX-License-Identifier: Apache-2.0
use super::*;

const LINE_TAG: u16 = 0x9cc8;
const ARC_TAG: u16 = 0x9cc6;
const POINT_TAG: u16 = 0x9cc2;

/// A lane payload under construction.
#[derive(Default)]
struct Lane {
    payload: Vec<u8>,
}

impl Lane {
    /// The ten bytes before a record, preceded by the array's count when the
    /// record is the first of its array.
    fn prefix(&mut self, count: Option<u32>, external: bool) {
        if let Some(count) = count {
            self.payload.extend_from_slice(&count.to_le_bytes());
        } else {
            self.payload.extend_from_slice(&[0; 4]);
        }
        self.payload
            .extend_from_slice(&u16::from(external).to_le_bytes());
        self.payload.extend_from_slice(&[0; 8]);
    }

    fn head(&mut self) -> usize {
        let at = self.payload.len();
        self.payload
            .extend_from_slice(&[0xff, 0xff, 0x1f, 0x00, 0x03]);
        self.payload.extend_from_slice(&[0xff; 8]);
        self.payload.extend_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        self.payload.resize(at + 48, 0);
        self.payload.extend_from_slice(&1.0f64.to_le_bytes());
        at
    }

    /// A local point at `(u, v)` metres naming `cells` as the curves that end at it.
    fn point(&mut self, count: Option<u32>, u: f64, v: f64, cells: &[(u16, u16)]) -> usize {
        self.prefix(count, false);
        let at = self.head();
        self.payload.resize(at + 64, 0);
        self.payload.extend_from_slice(&[0x1e, 0x00]);
        self.payload.extend_from_slice(&u.to_le_bytes());
        self.payload.extend_from_slice(&v.to_le_bytes());
        self.payload.extend_from_slice(&[0; 4]);
        for &(tag, slot) in cells {
            self.cell(tag, slot);
        }
        self.payload
            .extend_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0xff, 0xff]);
        self.payload.resize(self.payload.len() + 20, 0);
        at
    }

    /// A line record from point slot `start` to `end`.
    fn line(&mut self, count: Option<u32>, start: u16, end: u16) -> usize {
        self.prefix(count, false);
        let at = self.head();
        self.payload.resize(at + 64, 0);
        self.payload.extend_from_slice(&start.to_le_bytes());
        self.payload.extend_from_slice(&end.to_le_bytes());
        self.payload.extend_from_slice(&[0; 4]);
        self.payload.extend_from_slice(&(-1.0f64).to_le_bytes());
        self.payload.resize(at + 92, 0);
        at
    }

    /// An arc record from `start` to `end` about `centre`.
    fn arc(&mut self, count: Option<u32>, start: u16, end: u16, centre: u16) -> usize {
        self.prefix(count, false);
        let at = self.head();
        self.payload.resize(at + 64, 0);
        self.payload.extend_from_slice(&start.to_le_bytes());
        self.payload.extend_from_slice(&end.to_le_bytes());
        self.payload.extend_from_slice(&[0; 4]);
        self.payload.extend_from_slice(&(-1.0f64).to_le_bytes());
        self.payload.extend_from_slice(&1i32.to_le_bytes());
        self.payload.extend_from_slice(&centre.to_le_bytes());
        self.payload.extend_from_slice(&ARC_TAIL);
        self.payload.resize(at + 112, 0);
        at
    }

    fn cell(&mut self, tag: u16, slot: u16) {
        self.payload.extend_from_slice(&tag.to_le_bytes());
        self.payload.extend_from_slice(&slot.to_le_bytes());
        self.payload
            .extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
    }

    fn row(&mut self, code: u32, flag: u16, cells: &[(u16, u16)]) -> usize {
        let at = self.payload.len();
        self.payload.extend_from_slice(&code.to_le_bytes());
        self.payload.extend_from_slice(&[0x02, 0x00]);
        self.payload.extend_from_slice(&flag.to_le_bytes());
        self.payload
            .extend_from_slice(&[0x00, 0x00, 0xfe, 0xff, 0x00, 0x00]);
        for &(tag, slot) in cells {
            self.cell(tag, slot);
        }
        self.payload.resize(self.payload.len() + 40, 0);
        at
    }
}

/// Three lines in a chain and a separate arc: points 0..3 join lines 0..2,
/// arc 0 runs from point 4 to 5 about point 6. Rows: Horizontal(line 0),
/// Tangent(arc 0, line 2) with flag 1, Coincident(point 0, point 5).
fn chain_and_arc() -> (Lane, [usize; 3]) {
    let mut lane = Lane::default();
    lane.payload.resize(64, 0);
    lane.point(Some(7), 0.0, 0.0, &[(LINE_TAG, 0)]);
    lane.point(None, 0.01, 0.0, &[(LINE_TAG, 0), (LINE_TAG, 1)]);
    lane.point(None, 0.01, 0.01, &[(LINE_TAG, 1), (LINE_TAG, 2)]);
    lane.point(None, 0.0, 0.01, &[(LINE_TAG, 2)]);
    lane.point(None, 0.03, 0.0, &[(ARC_TAG, 0)]);
    lane.point(None, 0.02, 0.01, &[(ARC_TAG, 0)]);
    lane.point(None, 0.02, 0.0, &[]);
    lane.line(Some(3), 0, 1);
    lane.line(None, 1, 2);
    lane.line(None, 2, 3);
    lane.arc(Some(1), 4, 5, 6);
    lane.payload.resize(lane.payload.len() + 30, 0);
    let horizontal = lane.row(4, 0, &[(LINE_TAG, 0)]);
    let tangent = lane.row(6, 1, &[(ARC_TAG, 0), (LINE_TAG, 2)]);
    let coincident = lane.row(9, 0, &[(POINT_TAG, 0), (POINT_TAG, 5)]);
    (lane, [horizontal, tangent, coincident])
}

#[test]
fn a_block_reads_as_counted_arrays_and_its_rows() {
    let (lane, [horizontal, tangent, coincident]) = chain_and_arc();
    let sketches = lane_tables(&lane.payload);
    assert_eq!(sketches.len(), 1);
    let sketch = &sketches[0];
    assert_eq!(
        (sketch.points.len(), sketch.lines.len(), sketch.arcs.len()),
        (7, 3, 1)
    );
    let rows = sketch
        .rows
        .iter()
        .map(|row| (row.offset, row.code, row.flag, row.operands.len()))
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        vec![
            (horizontal, 4, 0, 1),
            (tangent, 6, 1, 2),
            (coincident, 9, 0, 2)
        ]
    );
    assert_eq!(
        sketch.rows[1].operands,
        vec![
            RowOperand {
                tag: HandleTag::Class(ARC_TAG),
                slot: 0
            },
            RowOperand {
                tag: HandleTag::Class(LINE_TAG),
                slot: 2
            },
        ]
    );
}

#[test]
fn point_cells_vote_the_curve_tags_and_the_last_tag_is_the_point() {
    let (lane, _) = chain_and_arc();
    let sketches = lane_tables(&lane.payload);
    let kinds = tag_kinds([(&lane.payload[..], sketches.as_slice())]);
    assert_eq!(
        kinds.get(&HandleTag::Class(LINE_TAG)),
        Some(&ArrayKind::Line)
    );
    assert_eq!(kinds.get(&HandleTag::Class(ARC_TAG)), Some(&ArrayKind::Arc));
    assert_eq!(
        kinds.get(&HandleTag::Class(POINT_TAG)),
        Some(&ArrayKind::Point)
    );
}

#[test]
fn a_cell_that_names_no_curve_end_does_not_vote() {
    let mut lane = Lane::default();
    lane.payload.resize(64, 0);
    // Points 0 and 3 name line 0, which ends at neither: no vote, so the line
    // tag stays unknown and two unknown tags leave the point tag unknown too.
    lane.point(Some(4), 0.0, 0.0, &[(LINE_TAG, 0)]);
    lane.point(None, 0.01, 0.0, &[]);
    lane.point(None, 0.02, 0.0, &[]);
    lane.point(None, 0.03, 0.0, &[(LINE_TAG, 0)]);
    lane.line(Some(1), 1, 2);
    lane.payload.resize(lane.payload.len() + 30, 0);
    lane.row(9, 0, &[(POINT_TAG, 0), (LINE_TAG, 0)]);
    let sketches = lane_tables(&lane.payload);
    let kinds = tag_kinds([(&lane.payload[..], sketches.as_slice())]);
    assert!(kinds.is_empty(), "{kinds:?}");
}

#[test]
fn a_tag_whose_votes_split_between_kinds_takes_none() {
    let mut lane = Lane::default();
    lane.payload.resize(64, 0);
    // Tag `LINE_TAG` names slot 0 at points 0, 1 and 2: line 0 ends at 0 and 1,
    // arc 0 at 2. Two votes of three for line are under the nine-tenths rule.
    lane.point(Some(5), 0.0, 0.0, &[(LINE_TAG, 0)]);
    lane.point(None, 0.01, 0.0, &[(LINE_TAG, 0)]);
    lane.point(None, 0.02, 0.0, &[(LINE_TAG, 0)]);
    lane.point(None, 0.03, 0.0, &[]);
    lane.point(None, 0.025, 0.0, &[]);
    lane.line(Some(1), 0, 1);
    lane.arc(Some(1), 2, 3, 4);
    let sketches = lane_tables(&lane.payload);
    let kinds = tag_kinds([(&lane.payload[..], sketches.as_slice())]);
    assert_eq!(kinds.get(&HandleTag::Class(LINE_TAG)), None, "{kinds:?}");
}

#[test]
fn a_count_past_the_markers_reads_no_array() {
    let mut lane = Lane::default();
    lane.payload.resize(64, 0);
    lane.point(Some(5), 0.0, 0.0, &[]);
    lane.point(None, 0.01, 0.0, &[]);
    assert!(lane_tables(&lane.payload).is_empty());
}

#[test]
fn a_row_header_with_another_flag_value_is_not_a_row() {
    let (mut lane, _) = chain_and_arc();
    let at = lane.row(5, 0, &[(LINE_TAG, 0)]);
    lane.payload[at + 6] = 2;
    let sketches = lane_tables(&lane.payload);
    assert!(sketches[0].rows.iter().all(|row| row.offset != at));
}

fn resolved(kind: ArrayKind, entity: &str) -> Resolved<'static> {
    let entity = SketchEntityId::mint(entity).expect("entity id");
    Resolved {
        kind: Some(kind),
        tag: HandleTag::Class(0x9cc0),
        slot: 0,
        record: Some(0),
        marker: None,
        locus: Some(SketchLocus::Entity(entity.clone())),
        entity: Some(entity),
    }
}

#[test]
fn confirmed_codes_type_and_others_do_not() {
    use SketchConstraintDefinitionInput as Def;
    let line = || resolved(ArrayKind::Line, "sldprt:model:sketch-entity#test:1");
    let other = || resolved(ArrayKind::Line, "sldprt:model:sketch-entity#test:2");
    let point = || resolved(ArrayKind::Point, "sldprt:model:sketch-entity#test:3");
    assert!(matches!(
        typed_definition(4, &[line()]),
        Some(Def::Horizontal { .. })
    ));
    assert!(matches!(
        typed_definition(5, &[line()]),
        Some(Def::Vertical { .. })
    ));
    assert!(matches!(
        typed_definition(7, &[line(), other()]),
        Some(Def::Parallel { .. })
    ));
    assert!(matches!(
        typed_definition(12, &[line(), point()]),
        Some(Def::Midpoint { .. })
    ));
    assert!(matches!(
        typed_definition(9, &[point(), line()]),
        Some(Def::PointOnObject { .. })
    ));
    // Fixed (17), OffsetEdge (16), UseEdge (32) and Patterned (57) are not typed.
    for code in [16, 17, 32, 57] {
        assert!(
            typed_definition(code, &[line(), other()]).is_none(),
            "{code}"
        );
    }
    // A confirmed code with operand kinds it does not take is not typed.
    assert!(typed_definition(7, &[line(), point()]).is_none());
    // Tangent lines, symmetric lines and concentric arcs are not typed.
    let arc = || resolved(ArrayKind::Arc, "sldprt:model:sketch-entity#test:4");
    let other_arc = || resolved(ArrayKind::Arc, "sldprt:model:sketch-entity#test:5");
    assert!(typed_definition(6, &[line(), other()]).is_none());
    assert!(typed_definition(6, &[arc(), line()]).is_some());
    assert!(typed_definition(11, &[line(), other(), line()]).is_none());
    assert!(matches!(
        typed_definition(11, &[point(), point(), line()]),
        Some(Def::Symmetric { .. })
    ));
    assert!(typed_definition(10, &[arc(), other_arc()]).is_none());
}

#[test]
fn rows_after_an_unsegmented_marker_belong_to_no_sketch() {
    let (mut lane, [horizontal, ..]) = chain_and_arc();
    // A marker record with no array count, then a row: the row follows a block
    // this reading did not segment, so it is not the sketch's.
    lane.prefix(None, false);
    lane.head();
    lane.payload.resize(lane.payload.len() + 120, 0);
    let stray = lane.row(5, 0, &[(LINE_TAG, 1)]);
    let sketches = lane_tables(&lane.payload);
    let offsets = sketches[0]
        .rows
        .iter()
        .map(|row| row.offset)
        .collect::<Vec<_>>();
    assert!(offsets.contains(&horizontal));
    assert!(!offsets.contains(&stray), "{offsets:?}");
}

#[test]
fn declaration_order_overrides_elimination() {
    let mut lane = Lane::default();
    // Point, arc, line handle classes declared in that order.
    for name in ["sgPointHandle", "sgArcHandle", "sgLineHandle"] {
        lane.payload.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
        lane.payload
            .extend_from_slice(&u16::try_from(name.len()).unwrap().to_le_bytes());
        lane.payload.extend_from_slice(name.as_bytes());
        lane.payload.extend_from_slice(&[0; 12]);
    }
    lane.payload.resize(lane.payload.len() + 64, 0);
    // The arc tag is voted; one more tag, above it, is used in a row.
    lane.point(Some(3), 0.02, 0.0, &[(ARC_TAG, 0)]);
    lane.point(None, 0.0, 0.02, &[(ARC_TAG, 0)]);
    lane.point(None, 0.0, 0.0, &[]);
    lane.arc(Some(1), 0, 1, 2);
    lane.payload.resize(lane.payload.len() + 30, 0);
    lane.row(4, 0, &[(LINE_TAG, 0)]);
    let sketches = lane_tables(&lane.payload);
    let kinds = tag_kinds([(&lane.payload[..], sketches.as_slice())]);
    assert_eq!(kinds.get(&HandleTag::Class(ARC_TAG)), Some(&ArrayKind::Arc));
    // Elimination alone would make it the point tag; the order makes it a line.
    assert_eq!(
        kinds.get(&HandleTag::Class(LINE_TAG)),
        Some(&ArrayKind::Line)
    );
}

mod projection {
    use super::*;
    use crate::records::SketchInputKind;
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::scalar::{Angle, Length};
    use cadmpeg_ir::sketches::SketchGeometry;

    const LANE: &str = "sldprt:feature-input:resolved-features#7";
    const FEATURE: &str = "feature";

    struct Fixture {
        lane: FeatureInputLane,
        sketch: SketchId,
        entities: Vec<SketchEntity>,
        rows: [usize; 4],
    }

    fn entity(
        id: &str,
        sketch: &SketchId,
        definition: SketchGeometryDefinition,
        marker: &str,
    ) -> SketchEntity {
        let geometry = SketchGeometry::try_from(definition).expect("geometry");
        let mut entity = SketchEntity::new(
            SketchEntityId::mint(format!("sldprt:model:sketch-entity#test:{id}")).expect("id"),
            sketch.clone(),
            geometry,
        );
        entity.native_ref = Some(marker.to_owned());
        entity
    }

    /// `chain_and_arc`'s lane with a marker for every record and an IR entity
    /// for every point and curve, all in one sketch of feature `FEATURE`.
    fn fixture() -> Fixture {
        let (mut lane_bytes, [horizontal, tangent, coincident]) = chain_and_arc();
        // Point 1 (an end of lines 0 and 1) on line 1.
        let on_line = lane_bytes.row(9, 0, &[(POINT_TAG, 1), (LINE_TAG, 1)]);
        let rows = [horizontal, tangent, coincident, on_line];
        let tables = lane_tables(&lane_bytes.payload);
        let table = &tables[0];
        let sketch = SketchId::mint("sldprt:model:sketch#test:1").expect("sketch id");
        let mut markers = Vec::new();
        let mut entities = Vec::new();
        let marker_id = |offset: usize| format!("sldprt:feature-input:sketch-entity#7:{offset}");
        let points = [
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [30.0, 0.0],
            [20.0, 10.0],
            [20.0, 0.0],
        ];
        for (slot, &offset) in table.points.iter().enumerate() {
            let kind = SketchInputKind::Point;
            let mut marker = SketchInputEntity::new(
                marker_id(offset),
                LANE,
                0,
                u64::try_from(offset).expect("offset fits u64"),
                kind,
            );
            marker.feature_ref = Some(FEATURE.into());
            markers.push(marker);
            let [u, v] = points[slot];
            entities.push(entity(
                &format!("p{slot}"),
                &sketch,
                SketchGeometryDefinition::Point {
                    position: Point2::new(u, v),
                },
                &marker_id(offset),
            ));
        }
        for (slot, &offset) in table.lines.iter().enumerate() {
            let mut marker = SketchInputEntity::new(
                marker_id(offset),
                LANE,
                0,
                u64::try_from(offset).expect("offset fits u64"),
                SketchInputKind::LineOrCircle,
            );
            marker.feature_ref = Some(FEATURE.into());
            markers.push(marker);
            let [a, b] = [points[slot], points[slot + 1]];
            entities.push(entity(
                &format!("l{slot}"),
                &sketch,
                SketchGeometryDefinition::Line {
                    start: Point2::new(a[0], a[1]),
                    end: Point2::new(b[0], b[1]),
                },
                &marker_id(offset),
            ));
        }
        for (slot, &offset) in table.arcs.iter().enumerate() {
            let mut marker = SketchInputEntity::new(
                marker_id(offset),
                LANE,
                0,
                u64::try_from(offset).expect("offset fits u64"),
                SketchInputKind::Arc,
            );
            marker.feature_ref = Some(FEATURE.into());
            markers.push(marker);
            entities.push(entity(
                &format!("a{slot}"),
                &sketch,
                SketchGeometryDefinition::Arc {
                    center: Point2::new(20.0, 0.0),
                    radius: Length::new(10.0).expect("radius"),
                    start_angle: Angle::new(0.0).expect("angle"),
                    end_angle: Angle::new(std::f64::consts::FRAC_PI_2).expect("angle"),
                },
                &marker_id(offset),
            ));
        }
        let lane = FeatureInputLane {
            id: LANE.into(),
            configuration: None,
            native_payload: lane_bytes.payload.into(),
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
        };
        Fixture {
            lane,
            sketch,
            entities,
            rows,
        }
    }

    fn project(fixture: &Fixture, constraints: &mut Vec<SketchConstraint>) {
        let sketches_by_feature = HashMap::from([(FEATURE, &fixture.sketch)]);
        project_relation_tables(
            constraints,
            &sketches_by_feature,
            &fixture.entities,
            std::slice::from_ref(&fixture.lane),
        );
    }

    fn by_row(constraints: &[SketchConstraint], offset: usize) -> &SketchConstraint {
        let id = row_id("7", offset);
        constraints
            .iter()
            .find(|constraint| constraint.id.as_str() == id)
            .expect("row projected")
    }

    #[test]
    fn a_flag_zero_row_types_and_a_flag_one_row_or_a_row_the_geometry_breaks_stays_native() {
        let fixture = fixture();
        let mut constraints = Vec::new();
        project(&fixture, &mut constraints);
        let [horizontal, tangent, coincident, on_line] = fixture.rows;
        // The point operand is a curve end, which binds the curve.
        assert!(matches!(
            by_row(&constraints, on_line).definition.kind(),
            SketchConstraintDefinitionInput::PointOnObject {
                point: SketchLocus::End(_) | SketchLocus::Start(_),
                ..
            }
        ));
        assert!(matches!(
            by_row(&constraints, horizontal).definition.kind(),
            SketchConstraintDefinitionInput::Horizontal { .. }
        ));
        assert!(matches!(
            by_row(&constraints, tangent).definition.kind(),
            SketchConstraintDefinitionInput::Native {
                native_flags: Some(1),
                ..
            }
        ));
        assert!(matches!(
            by_row(&constraints, coincident).definition.kind(),
            SketchConstraintDefinitionInput::Native { .. }
        ));
    }

    #[test]
    fn a_typed_projection_replaces_an_earlier_native_one() {
        let fixture = fixture();
        let mut constraints = Vec::new();
        // A first projection that knows no curve: every row is native.
        let bare = Fixture {
            entities: Vec::new(),
            lane: fixture.lane.clone(),
            sketch: fixture.sketch.clone(),
            rows: fixture.rows,
        };
        project(&bare, &mut constraints);
        let count = constraints.len();
        project(&fixture, &mut constraints);
        assert_eq!(constraints.len(), count);
        assert!(matches!(
            by_row(&constraints, fixture.rows[0]).definition.kind(),
            SketchConstraintDefinitionInput::Horizontal { .. }
        ));
    }

    #[test]
    fn an_entity_of_another_sketch_leaves_the_row_native() {
        let mut fixture = fixture();
        let other = SketchId::mint("sldprt:model:sketch#test:2").expect("sketch id");
        for entity in &mut fixture.entities {
            if entity.id().as_str().ends_with(":l0") {
                entity.sketch = other.clone();
            }
        }
        let mut constraints = Vec::new();
        project(&fixture, &mut constraints);
        assert!(matches!(
            by_row(&constraints, fixture.rows[0]).definition.kind(),
            SketchConstraintDefinitionInput::Native { .. }
        ));
    }
}
