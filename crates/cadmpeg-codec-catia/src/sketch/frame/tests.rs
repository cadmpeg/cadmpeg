// SPDX-License-Identifier: Apache-2.0
use super::{frame_program, placement, FrameProgram};
use crate::native::entity_record::{CatiaEntityRecord, CatiaEntityRecordBody};
use crate::native::{
    CatiaDesignObject, CatiaObjectEntity, CatiaObjectOwner, CatiaObjectRecord,
    CatiaObjectRecordReference, CatiaObjectRecordReferenceSource,
};
use crate::object_graph::{ObjectPayload, PayloadField};
use crate::test_support::{with_collection_limit, with_service_context};
use crate::unique_index::UniqueIndex;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::SketchPlacement;

const PROGRAM: [u8; 18] = [
    0x82, 0x86, 0xe8, 0xe0, 0x07, 0x37, 0x8a, 0x81, 0xe8, 0xe7, 0xe8, 0xe8, 0xe8, 0xe7, 0xe8, 0xe8,
    0xe8, 0xfe,
];

fn entity(payload: &[u8], id: u32) -> CatiaEntityRecord {
    CatiaEntityRecord {
        id: format!("entity-{id}"),
        object_graph: "graph".to_string(),
        object_record: format!("field-{id}"),
        ordinal: 0,
        byte_offset: 0,
        lead: 0,
        body: CatiaEntityRecordBody::Nested {
            definition_prefix: Vec::new(),
            definition_suffix: Vec::new(),
            value_payload: payload.to_vec(),
            record_suffix: Vec::new(),
        },
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

fn field(id: u32) -> CatiaObjectRecord {
    CatiaObjectRecord {
        id: format!("field-{id}"),
        parent: "graph".to_string(),
        design_object: Some("containing-group".to_string()),
        entity: Some(CatiaObjectEntity {
            record: format!("entity-{id}"),
            id,
        }),
        ordinal: 0,
        byte_offset: 0,
        byte_len: 0,
        lead: 0,
        head: Vec::new(),
        inline_body: None,
        owner: Some(CatiaObjectOwner::Entity(14)),
        class: None,
        storage: None,
        payload: ObjectPayload {
            size: 1,
            fields: vec![PayloadField::Terminator],
        },
        repeated_reference_schema_selection: None,
        references: Vec::new(),
    }
}

struct Fixture {
    declaration: CatiaObjectRecord,
    fields: Vec<CatiaObjectRecord>,
    entities: Vec<CatiaEntityRecord>,
    groups: Vec<CatiaDesignObject>,
}

impl Fixture {
    fn new() -> Self {
        let mut declaration = field(18);
        declaration
            .references
            .push(CatiaObjectRecordReference::from_parts(
                20,
                3,
                CatiaObjectRecordReferenceSource::ListItem {
                    list_payload_offset: 0,
                    item_ordinal: 0,
                },
                false,
                Some("field-20".to_string()),
                Some("containing-group".to_string()),
            ));
        let mut containing =
            crate::test_support::test_object_graph::design_object("containing-group", None);
        containing.owner_entity_id = 14;
        containing.fields = vec!["field-20".to_string(), "field-76".to_string()];
        let mut child = crate::test_support::test_object_graph::design_object(
            "child",
            Some("containing-group"),
        );
        child.owner_entity_id = 20;
        child.owner_record = Some("field-20".to_string());
        Self {
            declaration,
            fields: vec![field(20), field(76)],
            entities: vec![entity(&PROGRAM, 20), entity(&PROGRAM, 76)],
            groups: vec![containing, child],
        }
    }

    fn placement(&self) -> Option<SketchPlacement> {
        with_service_context(|ctx| {
            let records = UniqueIndex::collect(
                ctx,
                self.fields.iter().map(|v| (v.id.as_str(), v)),
                "test_records",
            )
            .expect("records");
            let entities = UniqueIndex::collect(
                ctx,
                self.entities.iter().map(|v| (v.id.as_str(), v)),
                "test_entities",
            )
            .expect("entities");
            let objects = UniqueIndex::collect(
                ctx,
                self.groups.iter().map(|v| (v.id.as_str(), v)),
                "test_objects",
            )
            .expect("objects");
            placement(ctx, &self.declaration, &records, &entities, &objects)
                .expect("placement resources")
        })
    }
}

#[test]
fn selects_containing_group_and_maps_horizontal_vertical_origin() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.placement(),
        Some(
            SketchPlacement::try_resolved(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
            )
            .expect("orthonormal frame")
        )
    );
}

#[test]
fn requires_unique_initial_roster_item_and_exact_identity_pairing() {
    for mutation in 0..9 {
        let mut fixture = Fixture::new();
        match mutation {
            0 => fixture.declaration.references.clear(),
            1 => fixture
                .declaration
                .references
                .push(fixture.declaration.references[0].clone()),
            2 => fixture.declaration.parent = "other-graph".to_string(),
            3 => fixture.fields[0].owner = Some(CatiaObjectOwner::Entity(20)),
            4 => fixture.entities[0].object_record = "other-field".to_string(),
            5 => fixture.entities[0].entity_id = 21,
            6 => fixture.entities[0].object_graph = "other-graph".to_string(),
            7 => {
                fixture.groups[0].fields.remove(0);
            }
            _ => fixture.fields.push(fixture.fields[0].clone()),
        }
        assert!(fixture.placement().is_none(), "mutation {mutation}");
    }
    for source in [
        CatiaObjectRecordReferenceSource::Field,
        CatiaObjectRecordReferenceSource::ListItem {
            list_payload_offset: 1,
            item_ordinal: 0,
        },
        CatiaObjectRecordReferenceSource::ListItem {
            list_payload_offset: 0,
            item_ordinal: 1,
        },
    ] {
        let mut fixture = Fixture::new();
        fixture.declaration.references[0] = CatiaObjectRecordReference::from_parts(
            20,
            3,
            source,
            false,
            Some("field-20".to_string()),
            Some("containing-group".to_string()),
        );
        assert!(fixture.placement().is_none());
    }
}

#[test]
fn conflicting_frame_copies_and_nonorthogonal_axes_remain_unresolved() {
    let mut other = PROGRAM;
    other[14] = 0xe7;
    let mut fixture = Fixture::new();
    fixture.entities[1] = entity(&other, 76);
    assert!(fixture.placement().is_none());
    other = PROGRAM;
    other[12] = 0xe7;
    other[13] = 0xe8;
    fixture.entities = vec![entity(&other, 20), entity(&other, 76)];
    assert!(fixture.placement().is_none());
    fixture.entities = vec![entity(&[], 20), entity(&PROGRAM, 76)];
    assert!(
        fixture.placement().is_none(),
        "copy cannot replace missing selected frame"
    );
}

#[test]
fn frame_program_requires_exact_tokens_and_nine_controls() {
    let controls = [0xe8, 0xe7, 0xe8, 0xe8, 0xe8, 0xe7, 0xe8, 0xe8, 0xe8];
    let parse = |bytes: &[u8]| {
        with_service_context(|ctx| frame_program(ctx, &entity(bytes, 20)).expect("tokens"))
    };
    assert_eq!(parse(&PROGRAM), FrameProgram::Controls(controls));
    for position in 0..PROGRAM.len() {
        let mut altered = PROGRAM;
        altered[position] = 0x00;
        assert_eq!(parse(&altered), FrameProgram::Absent, "position {position}");
    }
    assert_eq!(parse(&PROGRAM[..17]), FrameProgram::Absent);
    let mut extra = PROGRAM.to_vec();
    extra.insert(17, 0xe7);
    assert_eq!(parse(&extra), FrameProgram::Absent);
    let mut blob = vec![0xe5, 18, 0, 0, 0];
    blob.extend(PROGRAM);
    assert_eq!(parse(&blob), FrameProgram::Absent);
    let mut copies = PROGRAM.to_vec();
    copies.extend(PROGRAM);
    assert_eq!(parse(&copies), FrameProgram::Controls(controls));
    copies[18 + 14] = 0xe7;
    assert_eq!(parse(&copies), FrameProgram::Conflicting);
}

#[test]
fn frame_prefix_inside_binary64_packet_is_not_a_placement() {
    let mut payload = vec![0x91, 0x85, 0xe8, 0xe4, 0x07, 0x37, 0x83, 0x81, 0xe6];
    payload.extend(PROGRAM);
    let result = with_service_context(|ctx| frame_program(ctx, &entity(&payload, 20)))
        .expect("numeric packet boundary");
    assert_eq!(result, FrameProgram::Absent);
}

#[test]
fn frame_tokenization_refuses_before_unadmitted_allocation() {
    let result = with_collection_limit(0, |ctx| frame_program(ctx, &entity(&PROGRAM, 20)));
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
}
