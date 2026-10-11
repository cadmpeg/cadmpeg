// SPDX-License-Identifier: Apache-2.0
use super::{bind, diameter, frame_program, scalar};
use crate::decode::ModelingGraphScope;
use crate::entity_table::{RangeInterval, RangeIntervalPrefix, RangeIntervalSlot};
use crate::native::entity_record::{CatiaEntityRecord, CatiaEntityRecordBody};
use crate::native::{
    CatiaDefinitionSchemaSelection, CatiaEntitySchemaValue, CatiaEntityValueSchemaSelection,
    CatiaNative, CatiaObjectEntity, CatiaObjectGraph, CatiaObjectRecord, CatiaObjectStorage,
    CatiaRangeInterval, CatiaRangeNominal, CatiaRangeNominalFraming,
};
use crate::object_graph::{ObjectPayload, PayloadField};
use crate::test_support::{with_collection_limit, with_service_context};
use cadmpeg_ir::document::Model;

const PROGRAM: [u8; 19] = [
    0xfe, 0x9a, 0xdc, 0x72, 0xe8, 0xd4, 0x08, 0x37, 0x82, 0xfe, 0x94, 0xe2, 0xe5, 0xe8, 0xc0, 0x1c,
    0x37, 0x82, 0xfe,
];

fn entity(id: u32) -> CatiaEntityRecord {
    CatiaEntityRecord {
        id: format!("entity-{id}"),
        object_graph: "graph".into(),
        object_record: format!("record-{id}"),
        ordinal: 0,
        byte_offset: 0,
        lead: 2,
        body: CatiaEntityRecordBody::empty_nested(),
        definition_schema_selections: Vec::new(),
        entity_id: id,
        value_schema_selections: Vec::new(),
        object_production: None,
        value_production: None,
        range_interval: None,
        reference_signature: None,
        suffix: None,
        suffix_schema_selection: None,
    }
}

fn definition(name: &str) -> CatiaDefinitionSchemaSelection {
    CatiaDefinitionSchemaSelection {
        offset: 0,
        ordinal: 0,
        entry: Some(name.into()),
        name: Some(name.into()),
    }
}
fn selection(name: &str, offset: u64) -> CatiaEntityValueSchemaSelection {
    CatiaEntityValueSchemaSelection {
        offset,
        ordinal: 0,
        entry: name.into(),
        name: name.into(),
        encoded_value: Vec::new(),
        packets: Vec::new(),
    }
}
fn record(id: u32) -> CatiaObjectRecord {
    CatiaObjectRecord {
        id: format!("record-{id}"),
        parent: "graph".into(),
        design_object: None,
        entity: Some(CatiaObjectEntity {
            record: format!("entity-{id}"),
            id,
        }),
        ordinal: 0,
        byte_offset: 0,
        byte_len: 0,
        lead: 0x14,
        head: Vec::new(),
        inline_body: None,
        owner: None,
        class: None,
        storage: Some(CatiaObjectStorage {
            reference: 52,
            record: None,
            design_object: None,
        }),
        payload: ObjectPayload {
            size: 0,
            fields: Vec::new(),
        },
        repeated_reference_schema_selection: None,
        references: Vec::new(),
    }
}
fn suffix(position: usize, value: f64, modern: bool) -> Vec<u8> {
    let mut b = vec![
        if (6..9).contains(&position) {
            0x84
        } else {
            0x82
        },
        if position < 3 { 0x96 } else { 0x88 },
        0x82,
        if modern {
            if position < 3 {
                0xdf
            } else {
                0xed
            }
        } else if position < 3 {
            0xdc
        } else {
            0xea
        },
    ];
    if position == 5 {
        b.push(0xe8);
    } else {
        b.push(0xe6);
        b.extend(value.to_le_bytes());
    }
    b.extend([0x81, if modern { 0x92 } else { 0xdb }]);
    b
}
fn set_suffix(entity: &mut CatiaEntityRecord, bytes: Vec<u8>) {
    let CatiaEntityRecordBody::Nested { record_suffix, .. } = &mut entity.body else {
        panic!("nested fixture")
    };
    *record_suffix = bytes;
}

struct Fixture {
    native: CatiaNative,
    model: Model,
}
impl Fixture {
    fn new(modern: bool) -> Self {
        let mut range = entity(200);
        range.definition_schema_selections.push(definition("T"));
        range.value_schema_selections = vec![
            selection("ProfileList", 0),
            selection("GSMProject", 5),
            selection("Range", 10),
        ];
        range.range_interval = Some(CatiaRangeInterval {
            range: CatiaEntitySchemaValue {
                offset: 10,
                ordinal: 0,
                entry: "Range".into(),
                value: "Range".into(),
            },
            interval: RangeInterval {
                prefix: RangeIntervalPrefix::Compact {
                    value: 10,
                    width: 1,
                },
                slots: Some([
                    RangeIntervalSlot::Unset { offset: 20 },
                    RangeIntervalSlot::Unset { offset: 21 },
                ]),
            },
            nominal: Some(CatiaRangeNominal {
                framing: CatiaRangeNominalFraming::DCToken81DB,
                bits: 6.0_f64.to_bits(),
                evaluation_opcode_offset: 4,
            }),
            incoming_references: Vec::new(),
            incoming_storage_references: Vec::new(),
            project_target: None,
        });
        let mut frame = entity(100);
        frame.definition_schema_selections = vec![definition("_Elts"), definition("_Elts")];
        frame.value_schema_selections.push(selection("R", 0));
        let CatiaEntityRecordBody::Nested { value_payload, .. } = &mut frame.body else {
            panic!("nested fixture")
        };
        value_payload.extend([0x32, 0, 0, 0, 0]);
        value_payload.extend(PROGRAM);
        let mut frame_record = record(100);
        frame_record.lead = 0x54;
        frame_record.payload.size = 86;
        for i in 0..14 {
            frame_record.payload.fields.push(PayloadField::Reference {
                value: i + 1,
                offset: usize::try_from(i).expect("valid fixture") * 6,
            });
        }
        frame_record.payload.fields.extend([
            PayloadField::Atom {
                value: 129,
                offset: 84,
            },
            PayloadField::Terminator,
        ]);
        let mut entities = vec![range, frame];
        let mut records = vec![record(200), frame_record];
        let values = [2., 3., 4., 1., 0., 0., 0., 1., 0., 0., 0., 1.];
        for id in 1..=14 {
            let mut e = entity(id);
            e.value_schema_selections.push(selection("GSMProject", 0));
            if id <= 12 {
                set_suffix(
                    &mut e,
                    suffix(
                        usize::try_from(id - 1).expect("valid fixture"),
                        values[usize::try_from(id - 1).expect("valid fixture")],
                        modern,
                    ),
                );
            }
            entities.push(e);
            records.push(record(id));
        }
        let native = CatiaNative {
            entity_records: entities,
            object_graphs: vec![CatiaObjectGraph {
                id: "graph".into(),
                byte_offset: 0,
                byte_len: 0,
                finjpl_segment: None,
                outer_container: None,
                catalog_byte_offset: None,
                catalog: None,
                records,
            }],
            ..CatiaNative::default()
        };
        let mut model = Model::default();
        for i in 0..2 {
            model.surfaces.push(serde_json::from_value(serde_json::json!({"id":format!("synthetic:test:surface#{i}"),"geometry":{"kind":"cylinder","origin":{"x":2.,"y":3.,"z":-100.},"axis":{"x":0.,"y":0.,"z":1.},"ref_direction":{"x":1.,"y":0.,"z":0.},"radius":3.}})).expect("valid fixture"));
            model.faces.push(serde_json::from_value(serde_json::json!({"id":format!("synthetic:test:face#{i}"),"shell":"synthetic:test:shell#0","surface":format!("synthetic:test:surface#{i}"),"sense":"forward","loops":{"classification":"unspecified","loops":[]}})).expect("valid fixture"));
        }
        Self { native, model }
    }
    fn bind(&mut self) {
        with_service_context(|ctx| {
            bind(
                ctx,
                &mut self.native,
                &self.model,
                &ModelingGraphScope::Unscoped,
            )
        })
        .expect("valid fixture");
    }
    fn target(&self) -> Option<&super::ProjectRangeTarget> {
        self.native.entity_records[0]
            .range_interval
            .as_ref()
            .expect("valid fixture")
            .project_target
            .as_ref()
    }
}

#[test]
fn same_storage_frame_binds_two_faces_in_both_scalar_dialects() {
    for modern in [false, true] {
        let mut f = Fixture::new(modern);
        let before = f.native.entity_records[0].range_interval.clone();
        f.bind();
        let target = f.target().expect("valid fixture");
        assert_eq!(target.frame_entity, "entity-100");
        assert_eq!(
            target.faces.each_ref().map(cadmpeg_ir::ids::FaceId::as_str),
            ["synthetic:test:face#0", "synthetic:test:face#1"]
        );
        let mut after = f.native.entity_records[0].range_interval.clone();
        after.as_mut().expect("valid fixture").project_target = None;
        assert_eq!(before, after);
    }
}

#[test]
fn counted_terminal_list_retains_the_same_fourteen_reference_frame() {
    use crate::object_graph::ListItem;
    for count in [15, 14] {
        let mut f = Fixture::new(false);
        let frame = &mut f.native.object_graphs[0].records[1];
        frame.payload.size = 88;
        let items = (0..14)
            .map(|i| ListItem::Reference {
                value: u32::try_from(i + 1).expect("valid fixture"),
                offset: 2 + i * 6,
            })
            .collect();
        frame.payload.fields = vec![
            PayloadField::List {
                declared_count: count,
                items,
                offset: 0,
            },
            PayloadField::Terminator,
        ];
        f.bind();
        assert_eq!(f.target().is_some(), count == 15);
        if count == 15 {
            assert_eq!(
                f.target().expect("valid fixture").frame_entity,
                "entity-100"
            );
        }
    }
}

#[test]
fn exact_identity_storage_and_frame_uniqueness_are_required() {
    for mutation in 0..7 {
        let mut f = Fixture::new(false);
        match mutation {
            0 => {
                f.native.object_graphs[0].records[0]
                    .storage
                    .as_mut()
                    .expect("valid fixture")
                    .reference = 53;
            }
            1 => f.native.object_graphs[0].records[1].parent = "other-graph".into(),
            2 => {
                f.native.object_graphs[0].records[1]
                    .entity
                    .as_mut()
                    .expect("valid fixture")
                    .id = 101;
            }
            3 => f.native.entity_records[2].object_record = "missing".into(),
            4 => {
                let e = f.native.entity_records[2].clone();
                f.native.entity_records.push(e);
            }
            5 => {
                let mut e = f.native.entity_records[1].clone();
                e.id = "duplicate-frame".into();
                e.entity_id = 101;
                e.object_record = "duplicate-record".into();
                let mut r = f.native.object_graphs[0].records[1].clone();
                r.id = e.object_record.clone();
                r.entity = Some(CatiaObjectEntity {
                    record: e.id.clone(),
                    id: 101,
                });
                f.native.entity_records.push(e);
                f.native.object_graphs[0].records.push(r);
            }
            6 => f.native.object_graphs[0].records[1].lead = 0x14,
            _ => unreachable!(),
        }
        f.bind();
        assert!(f.target().is_none(), "mutation {mutation}");
    }
}

#[test]
fn terminal_reference_vector_and_complete_orthonormal_frame_are_required() {
    for mutation in 0..7 {
        let mut f = Fixture::new(false);
        match mutation {
            0 => {
                f.native.object_graphs[0].records[1].payload.fields[0] = PayloadField::Reference {
                    value: 1,
                    offset: 1,
                }
            }
            1 => {
                f.native.object_graphs[0].records[1].payload.fields[13] = PayloadField::Reference {
                    value: 1,
                    offset: 78,
                }
            }
            2 => {
                f.native.object_graphs[0].records[1].payload.fields[14] = PayloadField::Atom {
                    value: 128,
                    offset: 84,
                }
            }
            3 => set_suffix(&mut f.native.entity_records[5], suffix(3, 2., false)),
            4 => set_suffix(&mut f.native.entity_records[5], suffix(3, f64::MAX, false)),
            5 => set_suffix(&mut f.native.entity_records[13], suffix(11, -1., false)),
            6 => {
                let mut b = suffix(5, 0., false);
                b[4] = 0xe7;
                set_suffix(&mut f.native.entity_records[7], b);
            }
            _ => unreachable!(),
        }
        f.bind();
        assert!(f.target().is_none(), "mutation {mutation}");
    }
}

#[test]
fn geometry_requires_one_axis_two_carriers_and_one_face_per_carrier() {
    for mutation in 0..5 {
        let mut f = Fixture::new(false);
        match mutation {
            0 => {
                f.model.surfaces.pop();
            }
            1 => {
                let mut s = f.model.surfaces[0].clone();
                s.id = cadmpeg_ir::ids::SurfaceId::mint("synthetic:test:surface#extra")
                    .expect("valid fixture");
                f.model.surfaces.push(s);
            }
            2 => {
                f.model.faces[1].surface = f.model.faces[0].surface.clone();
            }
            3 => {
                f.model.faces.pop();
            }
            4 => {
                f.model.faces[1].id = f.model.faces[0].id.clone();
            }
            _ => unreachable!(),
        }
        f.bind();
        assert!(f.target().is_none(), "mutation {mutation}");
    }
}

#[test]
fn wrong_radius_displaced_axis_and_intersecting_axes_do_not_bind() {
    use cadmpeg_ir::geometry::analytic::CylinderSurface;
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};
    for mutation in 0..3 {
        let mut f = Fixture::new(false);
        let (point, axis, reference, radius) = match mutation {
            0 => (
                Point3::new(2., 3., 4.),
                Vector3::new(0., 0., 1.),
                Vector3::new(1., 0., 0.),
                4.,
            ),
            1 => (
                Point3::new(3., 3., 4.),
                Vector3::new(0., 0., 1.),
                Vector3::new(1., 0., 0.),
                3.,
            ),
            2 => (
                Point3::new(2., 3., 4.),
                Vector3::new(1., 0., 0.),
                Vector3::new(0., 1., 0.),
                3.,
            ),
            _ => unreachable!(),
        };
        f.model.surfaces[1].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::try_new(point, axis, reference, radius).expect("valid fixture"),
        ));
        f.bind();
        assert!(f.target().is_none(), "mutation {mutation}");
    }
}

#[test]
fn altered_range_and_frame_programs_remain_native() {
    let mut f = Fixture::new(false);
    f.native.entity_records[0]
        .range_interval
        .as_mut()
        .expect("valid fixture")
        .interval
        .slots
        .as_mut()
        .expect("valid fixture")[0] = RangeIntervalSlot::Binary64 {
        bits: 0,
        offset: 20,
    };
    assert!(diameter(&f.native.entity_records[0]).is_none());
    f.bind();
    assert!(f.target().is_none());
    for i in 0..PROGRAM.len() {
        if [1, 10].contains(&i) {
            continue;
        }
        let mut f = Fixture::new(false);
        let CatiaEntityRecordBody::Nested { value_payload, .. } =
            &mut f.native.entity_records[1].body
        else {
            panic!("nested")
        };
        value_payload[5 + i] ^= 1;
        assert!(!frame_program(&f.native.entity_records[1]), "byte {i}");
        f.bind();
        assert!(f.target().is_none());
    }
    let mut e = entity(1);
    e.value_schema_selections.push(selection("GSMProject", 0));
    set_suffix(&mut e, suffix(5, 0., false));
    assert_eq!(scalar(&e, 5), Some(0.));
    assert_eq!(scalar(&e, 4), None);
}

#[test]
fn frame_binding_refuses_unadmitted_index_collection() {
    let mut f = Fixture::new(false);
    assert!(matches!(
        with_collection_limit(0, |ctx| bind(
            ctx,
            &mut f.native,
            &f.model,
            &ModelingGraphScope::Unscoped
        )),
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
    assert!(f.target().is_none());
}

#[test]
fn unresolved_and_other_graph_scopes_leave_the_range_native() {
    for scope in [
        ModelingGraphScope::Unresolved,
        ModelingGraphScope::Scoped("other".into()),
    ] {
        let mut f = Fixture::new(false);
        with_collection_limit(0, |ctx| bind(ctx, &mut f.native, &f.model, &scope))
            .expect("valid fixture");
        assert!(f.target().is_none());
    }
}

#[test]
fn two_nearby_parallel_axes_are_not_one_target_axis() {
    use cadmpeg_ir::geometry::analytic::CylinderSurface;
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};
    let mut f = Fixture::new(false);
    for (surface, x) in f.model.surfaces.iter_mut().zip([2.00009, 1.99991]) {
        surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::try_new(
                Point3::new(x, 3., 4.),
                Vector3::new(0., 0., 1.),
                Vector3::new(1., 0., 0.),
                3.,
            )
            .expect("valid fixture"),
        ));
    }
    f.bind();
    assert!(f.target().is_none());
}
