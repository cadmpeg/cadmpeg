use super::super::*;
use super::*;

#[test]
fn decode_retains_strict_tiff_material_texture_assets() {
    let texture = [b'I', b'I', 42, 0, 8, 0, 0, 0, 0, 0];
    let malformed = [b'I', b'I', 42, 0, 40, 0, 0, 0, 0, 0];
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/materialsTif/AISI Steel 4340", texture.to_vec()),
        ("/Root/materialsTif/Truncated", malformed.to_vec()),
    ]);

    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("required invariant");
    let assets = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<super::super::MaterialTextureAsset>("material_texture_assets")
        .expect("required invariant");

    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].name, "AISI Steel 4340");
    assert_eq!(assets[0].byte_order, "little_endian");
    assert_eq!(assets[0].version, 42);
    assert_eq!(assets[0].first_ifd_offset, 8);
    assert_eq!(assets[0].byte_len, texture.len() as u64);
    assert_eq!(assets[0].sha256, cadmpeg_ir::hash::sha256_hex(&texture));
    assert_eq!(assets[0].source_entry, "/Root/materialsTif/AISI Steel 4340");
}

#[test]
fn decode_joins_qaf_material_names_to_texture_assets() {
    let texture = [b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 0];
    let qaf = br#"<?xml version="1.0" encoding="UTF-8"?>
<folderContents>
<folderProperties location="images/preview" unmappedLocation="images/preview"><createTime>2026-07-15T08:00:00</createTime><modifyTime>2026-07-15T08:00:01</modifyTime></folderProperties>
<folderProperties location="materialsTif/unmap$1" unmappedLocation="materialsTif/Carbon Fiber Harness Satin Coated"><createTime>2026-07-15T08:01:00</createTime><modifyTime>2026-07-15T08:02:00</modifyTime></folderProperties>
</folderContents>"#;
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/materialsTif/unmap$1", texture.to_vec()),
        ("/Root/qafmetadata", qaf.to_vec()),
    ]);

    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("required invariant");
    let namespace = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant");
    let assets = namespace
        .arena_as::<super::super::MaterialTextureAsset>("material_texture_assets")
        .expect("required invariant");
    let catalog = namespace
        .arena_as::<super::super::MaterialTextureCatalogEntry>("material_texture_catalog_entries")
        .expect("required invariant");

    assert_eq!(assets.len(), 1);
    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog[0].texture_asset, assets[0].id);
    assert_eq!(catalog[0].storage_path, "materialsTif/unmap$1");
    assert_eq!(
        catalog[0].material_path,
        "materialsTif/Carbon Fiber Harness Satin Coated"
    );
    assert_eq!(catalog[0].create_time, "2026-07-15T08:01:00");
    assert_eq!(catalog[0].modify_time, "2026-07-15T08:02:00");
    assert_eq!(catalog[0].source_entry, "/Root/qafmetadata");
}

#[test]
fn decode_rejects_ambiguous_nx_arrangement_table_atomically() {
    for arrangements in [
        br#"<Arrangements><Arrangement Default="YES" Name="Model"/><Arrangement Default="YES" Name="Exploded"/></Arrangements>"#.as_slice(),
        br#"<Arrangements><Arrangement Default="YES" Name="Model"/><Arrangement Default="NO" Name="Model"/></Arrangements>"#.as_slice(),
    ] {
        let file = prt_with_named_payloads(&[
            ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
            ("/Root/part/arrangements", arrangements.to_vec()),
        ]);
        let mut cur = Cursor::new(file);
        let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).expect("required invariant");
        assert!(result.ir().native.namespace("nx").is_none_or(|namespace| {
            namespace
                .arena_as::<super::super::Configuration>("configurations")
                .expect("required invariant")
                .is_empty()
        }));
        assert!(result.ir().model.configurations.is_empty());
    }
}

#[test]
fn decode_rejects_duplicate_nx_configuration_stream_paths_atomically() {
    let arrangements =
        br#"<Arrangements><Arrangement Default="YES" Name="Model"/></Arrangements>"#.to_vec();
    let attributes = br#"<UgAttributes version="4"><Attribute owner="part" pdmBased="false" utf8title="NX_Arrangement" utf8value="Model" version="3" type="StringAttributeType"/></UgAttributes>"#.to_vec();
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/part/arrangements", arrangements.clone()),
        ("/Root/part/arrangements", arrangements.clone()),
        ("/Root/part/attrs", attributes.clone()),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("required invariant");
    assert!(result.ir().model.configurations.is_empty());

    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/part/arrangements", arrangements),
        ("/Root/part/attrs", attributes.clone()),
        ("/Root/part/attrs", attributes),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("required invariant");
    assert_eq!(result.ir().model.configurations.len(), 1);
    assert!(result.ir().model.configurations[0].active.is_inactive());
    assert!(result.ir().model.configurations[0].bodies.is_unresolved());
    assert!(result.ir().native.namespace("nx").is_none_or(|namespace| {
        namespace
            .arena_as::<super::super::PartAttribute>("part_attributes")
            .expect("required invariant")
            .is_empty()
    }));
}

#[test]
fn assembly_metadata_lists_external_child_paths() {
    let mut cur = Cursor::new(assembly_with_external_paths());
    let result = NxCodec
        .decode(&mut cur, &DecodeOptions::default())
        .expect("required invariant");
    let attrs = &result.ir().source.as_ref().expect("source").attributes;
    assert_eq!(
        attrs.get("external_reference.0").map(String::as_str),
        Some("child.prt")
    );
    assert_eq!(
        attrs.get("external_reference.1").map(String::as_str),
        Some("nested/b.prt")
    );
    let references = result
        .ir()
        .native
        .namespace("nx")
        .expect("NX native namespace")
        .arena_as::<super::super::ExternalReference>("external_references")
        .expect("typed external references");
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].ordinal, 0);
    assert_eq!(references[0].path, "child.prt");
    assert_eq!(references[1].ordinal, 1);
    assert_eq!(references[1].path, "nested/b.prt");
    assert!(references[0].source_offset < references[1].source_offset);
}

#[test]
fn persistent_handle_identity_bridges_om_and_external_records() {
    let reference = super::super::ObjectReference {
        id: "nx:test:reference#0".into(),
        record: "nx:test:om-record#0".into(),
        object_id: Some(1),
        ordinal: 0,
        kind: super::super::ObjectReferenceKind::PersistentHandle,
        value: 0x1020_3040,
        target_record: None,
        source_entry: "om".into(),
        source_offset: 0,
    };
    let external = super::super::ExternalReferenceRecord {
        id: "nx:test:external-record#6".into(),
        record_id: 6,
        declared_count: 1,
        id_slots: [0; 4],
        handles: vec![0x1020_3040],
        closing_duplicate: true,
        prefix_byte_len: 31,
        tail_byte_len: 0,
        source_entry: "external".into(),
        source_offset: 10,
    };
    let control = super::super::DataBlockControlReference {
        id: "nx:test:control-reference#0".into(),
        data_block: "nx:test:control-block#0".into(),
        ordinal: 0,
        kind: super::super::ObjectReferenceKind::PersistentHandle,
        value: 0x1020_3040,
        source_offset: 20,
    };

    let tail_pair = super::super::ExternalReferenceTailReferencePair {
        id: "nx:test:tail-pair#0".into(),
        handle_set_record: external.id.clone(),
        ordinal: 0,
        persistent_handle: 0x5060_7080,
        tagged_reference: 7,
        source_offset: 30,
    };

    let handles =
        super::super::persistent_handles(&[reference], &[control], &[external], &[tail_pair]);

    assert_eq!(handles.len(), 2);
    assert_eq!(handles[0].records, ["nx:test:om-record#0"]);
    assert_eq!(handles[0].occurrence_count, 2);
    assert_eq!(handles[0].data_blocks, ["nx:test:control-block#0"]);
    assert_eq!(handles[0].external_records, ["nx:test:external-record#6"]);
    assert_eq!(handles[0].external_occurrence_count, 2);
    assert_eq!(handles[1].value, 0x5060_7080);
    assert_eq!(handles[1].external_records, ["nx:test:external-record#6"]);
    assert_eq!(handles[1].external_occurrence_count, 1);
}

#[test]
fn nx_control_handle_pairs_require_maximal_runs_of_exactly_two() {
    let reference = |ordinal: u32, offset: u64| super::super::DataBlockControlReference {
        id: format!("reference#{ordinal}"),
        data_block: "block#0".into(),
        ordinal,
        kind: super::super::ObjectReferenceKind::PersistentHandle,
        value: ordinal + 100,
        source_offset: offset,
    };
    let references = [
        reference(0, 10),
        reference(1, 15),
        reference(2, 30),
        reference(3, 35),
        reference(4, 40),
    ];
    let pairs = super::super::data_block_control_handle_pairs(&references);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].id, "nx:om-data-block-control:handle-pair#10");
    assert_eq!(pairs[0].first_reference, "reference#0");
    assert_eq!(pairs[0].second_reference, "reference#1");
    assert_eq!(pairs[0].first_handle, 100);
    assert_eq!(pairs[0].second_handle, 101);
}

#[test]
fn nx_object_record_handle_pairs_do_not_cross_records_or_long_runs() {
    let reference = |record: &str, ordinal: u32, offset: u64| super::super::ObjectReference {
        id: format!("{record}:reference#{ordinal}"),
        record: record.into(),
        object_id: Some(7),
        ordinal,
        kind: super::super::ObjectReferenceKind::PersistentHandle,
        value: ordinal + 100,
        target_record: None,
        source_entry: "om".into(),
        source_offset: offset,
    };
    let references = [
        reference("record#0", 0, 10),
        reference("record#0", 1, 15),
        reference("record#0", 2, 30),
        reference("record#0", 3, 35),
        reference("record#0", 4, 40),
        reference("record#1", 5, 20),
        reference("record#1", 6, 25),
    ];

    let pairs = super::super::object_record_handle_pairs(&references);
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[0].record, "record#0");
    assert_eq!(pairs[0].first_reference, "record#0:reference#0");
    assert_eq!(pairs[0].second_reference, "record#0:reference#1");
    assert_eq!(pairs[0].object_id, Some(7));
    assert_eq!(pairs[1].record, "record#1");
    assert_eq!(pairs[1].source_offset, 20);
}

#[test]
fn native_retains_rmfastload_table_and_member_words() {
    let container = container::scan_bytes(rmfastload_prt()).expect("required invariant");
    let entry_offset = container
        .entries
        .iter()
        .find(|entry| entry.name == "/Root/FastLoad/RMFastLoad")
        .and_then(|entry| entry.file_span)
        .expect("RMFastLoad span")
        .0;
    let (table, object_ids) =
        super::super::rmfastload_object_id_table(&container).expect("native RMFastLoad table");

    assert_eq!(table.id, "nx:rmfastload:object-id-table#0");
    assert_eq!(table.members.len(), 50);
    assert_eq!(table.raw_count, 50u32.to_le_bytes());
    assert_eq!(table.registry_source_offset, entry_offset);
    assert_eq!(
        table.source_offset,
        entry_offset + b"UGS::Solid::Topol".len() as u64
    );
    assert_eq!(object_ids[0].table, table.id);
    assert_eq!(object_ids[0].value, 1);
    assert_eq!(
        object_ids[0].stable_identity.as_deref(),
        Some("nx:rmfastload:object-id-table#0:value#1")
    );
    assert_eq!(object_ids[0].raw, 1u32.to_le_bytes());
    assert_eq!(object_ids[0].source_offset, table.source_offset + 4);
    assert_eq!(object_ids[49].ordinal, 49);
    assert_eq!(object_ids[49].value, 50);
    assert_eq!(object_ids[49].raw, 50u32.to_le_bytes());
    assert_eq!(table.members[49], object_ids[49].id);
    assert_eq!(
        super::super::rmfastload_target_object_id(&object_ids, 0),
        Some(object_ids[0].id.clone())
    );
    assert_eq!(
        super::super::rmfastload_target_object_id(&object_ids, 49),
        Some(object_ids[49].id.clone())
    );
    assert_eq!(
        super::super::rmfastload_target_object_id(&object_ids, 50),
        None
    );
}

#[test]
fn decode_selects_dominant_rmfastload_body() {
    let mut cur = Cursor::new(prt_with_two_bodies_and_rmfastload());
    let result = NxCodec
        .decode(&mut cur, &DecodeOptions::default())
        .expect("required invariant");
    let namespace = result.ir().native.namespace("nx").expect("NX namespace");
    let tables = namespace
        .arena_as::<super::super::RmFastLoadObjectIdTable>("rmfastload_object_id_tables")
        .expect("RMFastLoad tables");
    let object_ids = namespace
        .arena_as::<super::super::RmFastLoadObjectId>("rmfastload_object_ids")
        .expect("RMFastLoad object IDs");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].members.len(), 50);
    assert_eq!(object_ids.len(), 50);
    assert_eq!(object_ids[0].value, 1_000);
    assert_eq!(object_ids[49].value, 1_049);
    assert!(result.ir().model.bodies[0].id.0.starts_with("nx:s0:"));
    assert_eq!(result.ir().model.faces.len(), 50);
    assert_eq!(result.ir().model.surfaces.len(), 50);
    assert!(result
        .ir()
        .model
        .faces
        .iter()
        .all(|face| face.id.0.starts_with("nx:s0:")));
    assert!(result
        .ir()
        .model
        .surfaces
        .iter()
        .all(|surface| surface.id.0.starts_with("nx:s0:")));
    assert_eq!(
        result
            .ir()
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("active_body_selector"))
            .map(String::as_str),
        Some("rmfastload_object_id_membership")
    );
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(
        validation.findings.is_empty(),
        "findings: {:?}",
        validation.findings
    );
}

#[test]
fn data_block_column_index_tables_require_complete_mode_and_target_sequence() {
    use super::super::{
        data_block_column_index_tables, DataBlockLinkedIndexRow, DataBlockTargetIndexRow,
    };

    let linked = |id: &str, target: u32, mode: u8, offset: u64| DataBlockLinkedIndexRow {
        id: id.into(),
        section_ordinal: 2,
        ordinal: 0,
        first_index: 20,
        raw_first_index: vec![20],
        discriminator: 0x16,
        target_index: target,
        raw_target_index: vec![target as u8],
        indices: [5, 6, 7],
        raw_indices: [vec![5], vec![6], vec![7]],
        data_blocks: [
            format!("block#{target}"),
            "block#5".into(),
            "block#6".into(),
            "block#7".into(),
        ],
        flag: 3,
        mode,
        source_entry: "entry".into(),
        opening_data_block: format!("opening-block-{id}"),
        opening_block_offset: 8,
        source_offset: offset,
        first_index_source_offset: offset + 2,
        target_index_source_offset: offset + 7,
        index_source_offsets: [offset + 12, offset + 13, offset + 14],
    };
    let target = |id: &str, index: u32, mode: u8, offset: u64| DataBlockTargetIndexRow {
        id: id.into(),
        section_ordinal: 2,
        ordinal: 0,
        target_index: index,
        raw_target_index: vec![index as u8],
        indices: [5, 6, 7],
        raw_indices: [vec![5], vec![6], vec![7]],
        data_blocks: [
            format!("block#{index}"),
            "block#5".into(),
            "block#6".into(),
            "block#7".into(),
        ],
        mode,
        source_entry: "entry".into(),
        opening_data_block: format!("opening-block-{id}"),
        opening_block_offset: 8,
        source_offset: offset,
        target_index_source_offset: offset + 5,
        index_source_offsets: [offset + 10, offset + 11, offset + 12],
    };
    let linked_rows = [
        linked("opening", 63, 7, 100),
        linked("linked-59", 59, 4, 200),
        linked("linked-58", 58, 4, 225),
    ];
    let target_rows = [
        target("target-62", 62, 7, 125),
        target("target-61", 61, 7, 150),
        target("target-60", 60, 4, 175),
    ];

    let tables = data_block_column_index_tables(&linked_rows, &target_rows);
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].id, "nx:om-data-block-column-index-tables:table#2");
    assert_eq!(tables[0].opening_linked_row, "opening");
    assert_eq!(
        tables[0].target_rows,
        ["target-62", "target-61", "target-60"]
    );
    assert_eq!(tables[0].linked_rows, ["linked-59", "linked-58"]);
    assert_eq!(tables[0].first_target_index, 63);
    assert_eq!(tables[0].last_target_index, 58);
    assert_eq!(tables[0].source_offset, 100);

    let mut gap = target_rows.clone();
    gap[1].target_index = 60;
    assert!(data_block_column_index_tables(&linked_rows, &gap).is_empty());
    let mut incomplete_mode = target_rows.clone();
    incomplete_mode[2].mode = 7;
    assert!(data_block_column_index_tables(&linked_rows, &incomplete_mode).is_empty());
}

#[test]
fn external_reference_record_slots_resolve_atomically_in_the_same_stream() {
    use super::super::{
        external_reference_record_children, external_reference_record_string_uses,
        ExternalReference, ExternalReferenceRecord,
    };

    let references = (0..4)
        .map(|ordinal| ExternalReference {
            id: format!("reference#{ordinal}"),
            ordinal,
            path: format!("value-{ordinal}"),
            source_entry: "stream".into(),
            source_offset: 100 + u64::from(ordinal),
        })
        .collect::<Vec<_>>();
    let record = ExternalReferenceRecord {
        id: "record#7".into(),
        record_id: 7,
        declared_count: 2,
        id_slots: [0, 3, 1, 2],
        handles: vec![10, 20],
        closing_duplicate: true,
        prefix_byte_len: 40,
        tail_byte_len: 5,
        source_entry: "stream".into(),
        source_offset: 20,
    };
    let uses = external_reference_record_string_uses(std::slice::from_ref(&record), &references);
    assert_eq!(uses.len(), 4);
    assert_eq!(uses[0].id, "nx:external-reference:record-string-use#7-0");
    assert_eq!(
        uses.iter().map(|use_| use_.slot).collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    assert_eq!(
        uses.iter()
            .map(|use_| use_.string_index)
            .collect::<Vec<_>>(),
        [0, 3, 1, 2]
    );
    assert_eq!(uses[1].external_reference, "reference#3");
    assert_eq!(uses[1].source_offset, 31);
    let mut child_references = references.clone();
    child_references[0].path = "child.prt".into();
    let child_uses =
        external_reference_record_string_uses(std::slice::from_ref(&record), &child_references);
    let children = external_reference_record_children(
        std::slice::from_ref(&record),
        &child_references,
        &child_uses,
    );
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].external_record, record.id);
    assert_eq!(children[0].name_reference, "reference#0");
    assert_eq!(children[0].directory_reference, "reference#1");
    assert!(
        external_reference_record_children(std::slice::from_ref(&record), &references, &uses)
            .is_empty()
    );

    let mut out_of_range = record.clone();
    out_of_range.id_slots[2] = 4;
    assert!(external_reference_record_string_uses(&[out_of_range], &references).is_empty());
    let mut duplicate = references.clone();
    duplicate.push(references[0].clone());
    assert!(external_reference_record_string_uses(&[record], &duplicate).is_empty());
}
