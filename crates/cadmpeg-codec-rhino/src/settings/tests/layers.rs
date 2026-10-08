// SPDX-License-Identifier: Apache-2.0

use super::{metadata_table, parse_test_metadata, retained_limit_context};
use crate::chunks::ArchiveVersion;
use crate::loss::Diagnostics;
use crate::settings;
use crate::test_support::test_dump::{
    anonymous_chunk, class_userdata_with_payload, crc_chunk, crc_chunk_excluding, long_chunk,
    metadata_record, short_chunk, utf16_bytes,
};
use crate::wire::Uuid;
use std::sync::OnceLock;

#[test]
fn parses_layer_class_wrapper_and_rendering_chunk() {
    let archive = ArchiveVersion::V5;
    let mut payload = vec![0x18];
    payload.extend(0_i32.to_le_bytes());
    payload.extend(7_i32.to_le_bytes());
    payload.extend((-1_i32).to_le_bytes());
    payload.extend((-1_i32).to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    payload.extend([10, 20, 30, 255]);
    payload.extend(0_i16.to_le_bytes());
    payload.extend(0_i16.to_le_bytes());
    payload.extend(0.0_f64.to_le_bytes());
    payload.extend(1.0_f64.to_le_bytes());
    payload.extend(2_u32.to_le_bytes());
    payload.extend([b'L', 0, 0, 0]);
    payload.push(1);
    payload.extend((-1_i32).to_le_bytes());
    payload.extend([0, 0, 0, 255]);
    payload.extend(0.0_f64.to_le_bytes());
    payload.push(0);
    payload.extend([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let mut rendering = Vec::new();
    rendering.extend(1_i32.to_le_bytes());
    rendering.extend(0_i32.to_le_bytes());
    rendering.extend(0_i32.to_le_bytes());
    payload.extend(crc_chunk(archive, 0x4000_8000, &rendering));
    payload.extend([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    payload[0] = 0x1f;
    let mut linetype = Vec::new();
    linetype.extend(1_i32.to_le_bytes());
    linetype.extend(1_i32.to_le_bytes());
    linetype.extend(0_i32.to_le_bytes());
    linetype.extend(0_u32.to_le_bytes());
    linetype.extend(0_i32.to_le_bytes());
    linetype.extend([0; 16]);
    payload.push(33);
    payload.extend(crc_chunk(archive, 0x4000_8000, &linetype));
    payload.extend([34, 1]);
    let mut section_style = Vec::new();
    section_style.extend(1_i32.to_le_bytes());
    section_style.extend(1_i32.to_le_bytes());
    let model_attributes = crc_chunk(archive, 0x4000_8002, &[]);
    section_style.extend(&model_attributes);
    section_style.push(0);
    payload.push(35);
    #[allow(clippy::single_range_in_vec_init)] // The range is one checksum child.
    payload.extend(crc_chunk_excluding(
        archive,
        0x4000_8000,
        &section_style,
        std::slice::from_ref(&(8..8 + model_attributes.len())),
    ));
    payload.extend([36, 0, 37]);
    payload.extend(12_u32.to_le_bytes());
    payload.extend(
        "layer notes"
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes),
    );
    payload.push(0);
    let obsolete_idef_layer_settings = Uuid::from_canonical([
        0x11, 0xee, 0x2c, 0x1f, 0xf9, 0x0d, 0x4c, 0x6a, 0xa7, 0xcd, 0xec, 0x85, 0x32, 0xe1, 0xe3,
        0x2d,
    ])
    .to_wire();
    let obsolete_layer_settings = Uuid::from_canonical([
        0xbf, 0xb6, 0x3c, 0x09, 0x4b, 0xc7, 0x47, 0x27, 0x89, 0xbb, 0x7c, 0xc7, 0x54, 0x11, 0x82,
        0x00,
    ])
    .to_wire();
    let opennurbs5_application = Uuid::from_canonical([
        0xc8, 0xcd, 0xa5, 0x97, 0xd9, 0x57, 0x46, 0x25, 0xa4, 0xb3, 0xa0, 0xb5, 0x10, 0xfc, 0x30,
        0xd4,
    ])
    .to_wire();
    let obsolete_userdata = [
        class_userdata_with_payload(
            archive,
            obsolete_idef_layer_settings,
            opennurbs5_application,
            &[0xde, 0xad, 0xbe, 0xef],
        ),
        class_userdata_with_payload(
            archive,
            obsolete_layer_settings,
            opennurbs5_application,
            &[0xca, 0xfe, 0xba, 0xbe],
        ),
    ]
    .concat();
    let class_uuid = [
        0x13, 0x98, 0x80, 0x95, 0x85, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ];
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let class = long_chunk(
        archive,
        0x0002_7ffa,
        &[
            long_chunk(archive, 0x0002_fffb, &uuid_body),
            crc_chunk(archive, 0x0002_fffc, &payload),
            obsolete_userdata,
            short_chunk(archive, 0x8002_7fff, 0),
        ]
        .concat(),
    );
    let (data, record) = metadata_record(0x2000_8050, class);
    let mut wrapper_warnings = Diagnostics::new();
    let (class_descriptor, userdata) = crate::objects::parse_class_wrapper_with_userdata(
        &cadmpeg_test_support::service_decode_context(),
        &data,
        record.body(),
        archive,
        &mut wrapper_warnings,
    )
    .expect("required invariant");
    assert_eq!(class_descriptor.class_data_range.len(), payload.len());
    assert_eq!(userdata.len(), 2);
    let table = metadata_table(0x1000_0011, data.len(), vec![record]);
    let mut warnings = Diagnostics::new();
    let metadata = parse_test_metadata(&data, archive, &[table], &mut warnings);
    assert_eq!(metadata.layers.len(), 1, "{warnings:?}");
    assert_eq!(metadata.layers[0].index, 7);
    assert_eq!(metadata.layers[0].iges_level, None);
    assert_eq!(metadata.layers[0].render_material_index, -1);
    assert_eq!(metadata.layers[0].color, [10, 20, 30, 255]);
    assert_eq!(metadata.layers[0].name, "L");
    assert_eq!(
        metadata.layers[0].description.as_deref(),
        Some("layer notes")
    );
    assert!(metadata.layers[0].visible);
    assert!(!metadata.layers[0].locked);
    assert_eq!(metadata.layers[0].visible_in_new_details, Some(true));
    assert_eq!(
        metadata.layers[0]
            .embedded_linetype
            .as_ref()
            .map(|value| value.version),
        Some((1, 1))
    );
    assert_eq!(
        metadata.layers[0]
            .embedded_section_style
            .as_ref()
            .map(|value| value.version),
        Some((1, 1))
    );
    assert_eq!(metadata.opaque_records.len(), 1);
    assert!(
        warnings
            .iter()
            .all(|warning| warning.contains(LAYER_PARENT_DIALECT)),
        "{warnings:?}"
    );

    let mut future_payload = payload.clone();
    future_payload.pop();
    future_payload.extend([0xfe, 0xaa, 0xbb, 0, 0xde]);
    let future_class = long_chunk(
        archive,
        0x0002_7ffa,
        &[
            long_chunk(archive, 0x0002_fffb, &uuid_body),
            crc_chunk(archive, 0x0002_fffc, &future_payload),
            short_chunk(archive, 0x8002_7fff, 0),
        ]
        .concat(),
    );
    let (future_data, future_record) = metadata_record(0x2000_8050, future_class);
    let future_table = metadata_table(0x1000_0011, future_data.len(), vec![future_record.clone()]);
    let mut future_warnings = Diagnostics::new();
    let future = parse_test_metadata(&future_data, archive, &[future_table], &mut future_warnings);
    assert_eq!(future.layers.len(), 1, "{future_warnings:?}");
    assert_eq!(future.layers[0].extension_items, vec![33, 34, 35, 36, 37]);
    assert_eq!(future.opaque_records.len(), 1);
}

/// Marker of the diagnostic raised for an unstamped layer record.
const LAYER_PARENT_DIALECT: &str = "layer parent link and expanded state were not read";

fn layer_metadata_with_extension(extension: &[u8]) -> settings::DocumentMetadata {
    let (metadata, warnings) = layer_metadata(extension, None);
    assert!(
        warnings
            .iter()
            .all(|warning| warning.contains(LAYER_PARENT_DIALECT)),
        "{warnings:?}"
    );
    metadata
}

/// Parses one layer record, with the writer-version stamp under test control.
///
/// A `Some` stamp is delivered the way an archive delivers it: a short
/// writer-version record in a properties table ahead of the layer table. The
/// payload follows the stamp: a stamped archive carries the parent link and the
/// expanded flag that the stamped reading consumes, an unstamped one does not,
/// so each arm parses a record its own reading admits.
fn layer_metadata(
    extension: &[u8],
    writer_version: Option<i64>,
) -> (settings::DocumentMetadata, Diagnostics) {
    layer_metadata_with_record_count(extension, writer_version, 1)
}

fn layer_metadata_with_record_count(
    extension: &[u8],
    writer_version: Option<i64>,
    record_count: usize,
) -> (settings::DocumentMetadata, Diagnostics) {
    layer_metadata_with_record_count_and_id(extension, writer_version, record_count, [0; 16])
}

fn layer_metadata_with_record_count_and_id(
    extension: &[u8],
    writer_version: Option<i64>,
    record_count: usize,
    id: [u8; 16],
) -> (settings::DocumentMetadata, Diagnostics) {
    let (data, tables) = layer_fixture(extension, writer_version, record_count, id, &[]);
    let mut warnings = Diagnostics::new();
    let metadata = parse_test_metadata(&data, ArchiveVersion::V8, &tables, &mut warnings);
    (metadata, warnings)
}

pub(super) fn layer_fixture(
    extension: &[u8],
    writer_version: Option<i64>,
    record_count: usize,
    id: [u8; 16],
    userdata: &[u8],
) -> (Vec<u8>, Vec<crate::container::Table>) {
    let archive = ArchiveVersion::V8;
    let mut payload = vec![0x1f];
    payload.extend(0_i32.to_le_bytes());
    payload.extend(7_i32.to_le_bytes());
    payload.extend((-1_i32).to_le_bytes());
    payload.extend((-1_i32).to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    payload.extend([0, 0, 0, 255]);
    payload.extend(0_i16.to_le_bytes());
    payload.extend(0_i16.to_le_bytes());
    payload.extend(0.0_f64.to_le_bytes());
    payload.extend(1.0_f64.to_le_bytes());
    payload.extend(utf16_bytes("L"));
    payload.push(1);
    payload.extend((-1_i32).to_le_bytes());
    payload.extend([0, 0, 0, 255]);
    payload.extend(0.0_f64.to_le_bytes());
    payload.push(0);
    payload.extend(id);
    if writer_version.is_some() {
        payload.extend([0x44; 16]);
        payload.push(1);
    }
    payload.extend(crc_chunk(
        archive,
        0x4000_8000,
        &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    ));
    payload.extend([0; 16]);
    payload.extend_from_slice(extension);

    let class_uuid = [
        0x13, 0x98, 0x80, 0x95, 0x85, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22,
        0xf0,
    ];
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let class = long_chunk(
        archive,
        0x0002_7ffa,
        &[
            long_chunk(archive, 0x0002_fffb, &uuid_body),
            crc_chunk(archive, 0x0002_fffc, &payload),
            userdata.to_vec(),
            short_chunk(archive, 0x8002_7fff, 0),
        ]
        .concat(),
    );
    let (data, record) = metadata_record(0x2000_8050, class);
    let table = metadata_table(0x1000_0011, data.len(), vec![record; record_count]);
    let mut tables = Vec::new();
    if let Some(value) = writer_version {
        tables.push(metadata_table(
            0x1000_0014,
            0,
            vec![crate::container::Record::short(0xa000_0026, 0..0, value)],
        ));
    }
    tables.push(table);
    (data, tables)
}

/// Whether a ladder fixture's layer records carry the writer-version stamp.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LayerStamp {
    WriterVersion,
    Unstamped,
}

/// Which singleton-bearing tables a metadata ladder fixture carries, and
/// how its layer records are stamped.
#[derive(Clone, Copy)]
struct MetadataTables {
    properties: bool,
    settings: bool,
    layers: bool,
    stamp: LayerStamp,
}

const ALL_METADATA_TABLES: MetadataTables = MetadataTables {
    properties: true,
    settings: true,
    layers: true,
    stamp: LayerStamp::WriterVersion,
};

fn metadata_limit_operations(
    dimension: cadmpeg_core::decode::ResourceDimension,
    source_id: [u8; 16],
    extension: &[u8],
    userdata: &[u8],
) -> Vec<&'static str> {
    metadata_limit_operations_for(
        dimension,
        source_id,
        extension,
        userdata,
        ALL_METADATA_TABLES,
    )
}

fn metadata_limit_operations_for(
    dimension: cadmpeg_core::decode::ResourceDimension,
    source_id: [u8; 16],
    extension: &[u8],
    userdata: &[u8],
    carried: MetadataTables,
) -> Vec<&'static str> {
    let (data, mut layer_tables) = layer_fixture(
        extension,
        (carried.stamp == LayerStamp::WriterVersion).then_some(200_912_010),
        1,
        source_id,
        userdata,
    );
    if carried.stamp == LayerStamp::WriterVersion {
        layer_tables.remove(0);
    }
    let mut tables = Vec::new();
    if carried.properties {
        tables.push(metadata_table(
            super::super::PROPERTIES,
            0,
            vec![
                crate::container::Record::short(super::super::WRITER_VERSION, 0..0, 200_912_010),
                crate::container::Record::long(super::super::PREVIEW, 0..0, 0..0),
            ],
        ));
    }
    if carried.settings {
        tables.push(metadata_table(
            super::super::SETTINGS,
            0,
            vec![
                crate::container::Record::short(super::super::CURRENT_LAYER, 0..0, 3),
                crate::container::Record::long(0x2000_ffff, 0..0, 0..0),
            ],
        ));
    }
    if carried.layers {
        tables.append(&mut layer_tables);
    }
    let mut limit = 0_u64;
    let mut operations = Vec::new();
    for _ in 0..1024 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        match dimension {
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = limit;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = limit;
            }
            other => panic!("unsupported metadata test dimension: {other:?}"),
        }
        let ctx = retained_limit_context(&data, &arena, &policy);
        match settings::parse_metadata(
            &ctx,
            &data,
            ArchiveVersion::V8,
            &tables,
            &mut Diagnostics::new(),
        ) {
            Ok(metadata) => {
                assert_eq!(metadata.layers.len(), usize::from(carried.layers));
                return operations;
            }
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == dimension =>
            {
                operations.push(refusal.operation);
                let next = refusal.used + refusal.additional;
                limit = if dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes {
                    // A hash table's growth is charged as a transient bound that
                    // is released after the insertion, so jumping to the refused
                    // total would step over later, smaller refusals; advance one
                    // byte.
                    limit + 1
                } else {
                    next.max(limit + 1)
                };
            }
            Err(error) => panic!("unexpected metadata error: {error}"),
        }
    }
    panic!("metadata limit ladder did not terminate");
}

fn metadata_collection_operations() -> &'static [&'static str] {
    static OPERATIONS: OnceLock<Vec<&'static str>> = OnceLock::new();
    OPERATIONS.get_or_init(|| {
        metadata_limit_operations(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            [0x44; 16],
            &[0],
            &[],
        )
    })
}

/// The layer UUID table is the first scoped table a decode grows, so it
/// refuses first; a layer without a UUID grows no UUID table, so the index
/// count copy refuses first. The ladder runs over both.
fn metadata_materialized_operations() -> &'static [&'static str] {
    static OPERATIONS: OnceLock<Vec<&'static str>> = OnceLock::new();
    OPERATIONS.get_or_init(|| {
        let unstamped_layers = MetadataTables {
            properties: false,
            settings: false,
            layers: true,
            stamp: LayerStamp::Unstamped,
        };
        [
            (ALL_METADATA_TABLES, [0x44; 16]),
            (unstamped_layers, [0x44; 16]),
            (unstamped_layers, [0; 16]),
        ]
        .into_iter()
        .flat_map(|(carried, source_id)| {
            metadata_limit_operations_for(
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
                source_id,
                &[0],
                &[],
                carried,
            )
        })
        .collect()
    })
}

fn metadata_opaque_operations() -> &'static [&'static str] {
    static OPERATIONS: OnceLock<Vec<&'static str>> = OnceLock::new();
    OPERATIONS.get_or_init(|| {
        metadata_limit_operations(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            [0; 16],
            &[0],
            &[],
        )
    })
}

fn metadata_extension_operations() -> &'static [&'static str] {
    static OPERATIONS: OnceLock<Vec<&'static str>> = OnceLock::new();
    OPERATIONS.get_or_init(|| {
        let mut extension = vec![37];
        extension.extend(utf16_bytes("description"));
        extension.push(0);
        metadata_limit_operations(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            [0x44; 16],
            &extension,
            &[],
        )
    })
}

fn metadata_userdata_operations() -> &'static [&'static str] {
    static OPERATIONS: OnceLock<Vec<&'static str>> = OnceLock::new();
    OPERATIONS.get_or_init(|| {
        let archive = ArchiveVersion::V8;
        let mut entry = super::super::LAYER_PER_VIEWPORT_ID.to_le_bytes().to_vec();
        entry.extend(Uuid::from_canonical([1; 16]).to_wire());
        let mut outer_body = 1_i32.to_le_bytes().to_vec();
        outer_body.extend(anonymous_chunk(archive, 2, &entry));
        let userdata = class_userdata_with_payload(
            archive,
            settings::LAYER_EXTENSIONS.to_wire(),
            [0; 16],
            &outer_body,
        );
        metadata_limit_operations(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            [0x44; 16],
            &[0],
            &userdata,
        )
    })
}

macro_rules! metadata_limit_test {
    ($name:ident, $operations:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let operations = $operations();
            assert!(operations.contains(&$operation), "reached {operations:?}");
        }
    };
}

metadata_limit_test!(
    property_previews_refuse_collection_limit,
    metadata_collection_operations,
    "Rhino property previews"
);
metadata_limit_test!(
    unsupported_settings_refuse_collection_limit,
    metadata_collection_operations,
    "Rhino unsupported settings"
);
metadata_limit_test!(
    layer_uuid_keys_refuse_collection_limit,
    metadata_collection_operations,
    "Rhino layer UUID keys"
);
metadata_limit_test!(
    metadata_layers_refuse_collection_limit,
    metadata_collection_operations,
    "Rhino metadata layers"
);
metadata_limit_test!(
    layer_index_counts_refuse_collection_limit,
    metadata_collection_operations,
    "Rhino layer index counts"
);
metadata_limit_test!(
    layer_parent_counts_refuse_collection_limit,
    metadata_collection_operations,
    "Rhino layer parent counts"
);
metadata_limit_test!(
    metadata_opaque_records_refuse_collection_limit,
    metadata_opaque_operations,
    "Rhino metadata opaque records"
);
metadata_limit_test!(
    layer_uuid_keys_refuse_materialized_limit,
    metadata_materialized_operations,
    "Rhino layer UUID keys"
);
metadata_limit_test!(
    layer_index_counts_refuse_materialized_limit,
    metadata_materialized_operations,
    "Rhino layer index counts"
);
metadata_limit_test!(
    layer_extension_items_refuse_collection_limit,
    metadata_extension_operations,
    "Rhino layer extension items"
);
metadata_limit_test!(
    layer_userdata_resource_refusal_propagates,
    metadata_userdata_operations,
    "Rhino layer extension entries"
);

/// The layer parent link rests on the stamp, so the loss follows the stamp.
#[test]
fn unstamped_layer_charges_the_parent_link_stamp_loss() {
    // A single zero closes the extension-item chain, so both arms read a whole
    // record and the difference between them is only the stamp.
    let (unstamped_metadata, unstamped) = layer_metadata(&[0], None);
    assert_eq!(unstamped_metadata.layers.len(), 1, "{unstamped:?}");
    assert_eq!(
        unstamped_metadata.layers[0]
            .hierarchy
            .map(|hierarchy| hierarchy.parent_id),
        None
    );
    assert_eq!(
        unstamped_metadata.layers[0]
            .hierarchy
            .map(|hierarchy| hierarchy.expanded),
        None
    );
    assert!(
        unstamped_metadata
            .losses
            .iter()
            .any(|loss| loss.message.contains(LAYER_PARENT_DIALECT)),
        "{:?}",
        unstamped_metadata.losses
    );

    // The stamped arm must read a layer, or its silence proves nothing.
    let (stamped_metadata, stamped) = layer_metadata(&[0], Some(200_912_010));
    assert_eq!(stamped_metadata.layers.len(), 1, "{stamped:?}");
    assert_eq!(
        stamped_metadata.layers[0]
            .hierarchy
            .map(|hierarchy| hierarchy.parent_id),
        Some(Uuid::from_canonical([0x44; 16]))
    );
    assert_eq!(
        stamped_metadata.layers[0]
            .hierarchy
            .map(|hierarchy| hierarchy.expanded),
        Some(true)
    );
    assert!(
        !stamped
            .iter()
            .any(|warning| warning.contains(LAYER_PARENT_DIALECT)),
        "{stamped:?}"
    );
}

#[test]
fn unstamped_layer_loss_refuses_collection_limit() {
    let (data, tables) = layer_fixture(&[0], None, 1, [0; 16], &[]);
    let mut limit = 0_u64;
    for _ in 0..32 {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let ctx = retained_limit_context(&data, &arena, &policy);
        match settings::parse_metadata(
            &ctx,
            &data,
            ArchiveVersion::V8,
            &tables,
            &mut Diagnostics::new(),
        ) {
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.operation == "Rhino layer losses" =>
            {
                return
            }
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) => {
                limit = (refusal.used + refusal.additional).max(limit + 1);
            }
            other => panic!("expected a layer loss refusal, got {other:?}"),
        }
    }
    panic!("layer loss boundary was not reached");
}

#[test]
fn duplicate_layer_indexes_are_preserved_and_reported() {
    let (metadata, warnings) = layer_metadata_with_record_count(&[0], None, 2);

    assert_eq!(metadata.layers.len(), 2, "{warnings:?}");
    assert!(metadata.layers.iter().all(|layer| layer.index == 7));
    assert!(
        warnings.iter().any(|warning| {
            warning.contains(
                "duplicate layer index 7 occurs 2 times; raw indexes preserved and object bindings withheld",
            )
        }),
        "{warnings:?}"
    );
}

#[test]
fn nil_layer_uuid_is_source_absence_and_is_retained_per_record() {
    let (metadata, warnings) = layer_metadata_with_record_count_and_id(&[0], None, 2, [0; 16]);

    assert_eq!(metadata.layers.len(), 2, "{warnings:?}");
    assert!(metadata.layers.iter().all(|layer| layer.id.is_none()));
    assert_eq!(metadata.opaque_records.len(), 2);
    assert!(!warnings
        .iter()
        .any(|warning| warning.contains("duplicate layer UUID")));
}

#[test]
fn duplicate_non_nil_layer_uuids_remain_ambiguous() {
    let id = [0x44; 16];
    let (metadata, warnings) = layer_metadata_with_record_count_and_id(&[0], None, 2, id);

    assert_eq!(metadata.layers.len(), 2, "{warnings:?}");
    assert!(metadata
        .layers
        .iter()
        .all(|layer| layer.id == Some(Uuid::from_canonical(id))));
    assert!(warnings.iter().any(|warning| {
        warning.code == Some(crate::loss::RhinoLossCode::DuplicateRecordResolved)
            && warning.contains("duplicate layer UUID")
    }));
    assert!(metadata.opaque_records.is_empty());
}

#[test]
fn duplicate_layer_parent_uuid_is_reported_as_ambiguous() {
    let (mut metadata, _) = layer_metadata(&[0], Some(200_912_010));
    let parent = Uuid::from_canonical([0x44; 16]);
    metadata.layers[0].id = Some(parent);
    let mut duplicate = metadata.layers[0].clone();
    duplicate.index = 8;
    metadata.layers.push(duplicate);

    let mut warnings = Diagnostics::new();
    super::super::report_layer_parent_references(
        &cadmpeg_test_support::service_decode_context(),
        &metadata.layers,
        &mut warnings,
    )
    .expect("parent count map fits service profile");

    assert!(
        warnings.iter().any(|warning| {
            warning.code == Some(crate::loss::RhinoLossCode::DuplicateRecordResolved)
                && warning.contains("ambiguous parent UUID")
                && warning.contains("2 layer records")
        }),
        "{warnings:?}"
    );
}

#[test]
fn missing_layer_parent_diagnostic_refuses_collection_limit() {
    let (mut metadata, _) = layer_metadata(&[0], Some(200_912_010));
    metadata.layers[0].id = None;
    metadata.layers[0].hierarchy = Some(settings::LayerHierarchy {
        parent_id: Uuid::from_canonical([0x44; 16]),
        expanded: false,
    });
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let refusal =
        super::super::report_layer_parent_references(&ctx, &metadata.layers, &mut Diagnostics::new())
            .expect_err("one missing-parent warning exceeds zero collection items");
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "Rhino diagnostics"
    ));
    let mut warnings = Diagnostics::new();
    super::super::report_layer_parent_references(
        &cadmpeg_test_support::service_decode_context(),
        &metadata.layers,
        &mut warnings,
    )
    .expect("service profile admits warning");
    assert_eq!(warnings.iter().count(), 1);
}

fn layer_metadata_with_description(description: &str) -> settings::DocumentMetadata {
    let mut extension = vec![37];
    extension.extend(utf16_bytes(description));
    extension.push(0);
    layer_metadata_with_extension(&extension)
}

fn layer_text_refusal(extension: &[u8], limit: u64) -> cadmpeg_core::CodecError {
    let (data, tables) = layer_fixture(extension, None, 1, [0; 16], &[]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let ctx = retained_limit_context(&data, &arena, &policy);
    settings::parse_metadata(
        &ctx,
        &data,
        ArchiveVersion::V8,
        &tables,
        &mut Diagnostics::new(),
    )
    .expect_err("layer text exceeds retained limit")
}

#[test]
fn layer_name_refuses_retained_limit() {
    let error = layer_text_refusal(&[0], 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.operation == "Rhino layer name")
    );
}

#[test]
fn layer_description_refuses_retained_limit() {
    let mut extension = vec![37];
    extension.extend(utf16_bytes(" description "));
    extension.push(0);
    let error = layer_text_refusal(
        &extension,
        crate::test_support::retained_limit_at("Rhino layer loss text", 0, |cap| {
            match layer_text_refusal(&extension, cap) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit,
                error => panic!("unexpected resource refusal: {error:?}"),
            }
        }),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.operation == "Rhino layer loss text")
    );
    let mut limit = 1;
    let mut found_description = false;
    for _ in 0..32 {
        let cadmpeg_core::CodecError::ResourceLimit(refusal) =
            layer_text_refusal(&extension, limit)
        else {
            panic!("expected a retained resource refusal at limit {limit}");
        };
        if refusal.operation == "Rhino layer description" {
            found_description = true;
            break;
        }
        let next = refusal
            .used
            .checked_add(refusal.additional)
            .expect("fixture budget fits");
        assert!(next > limit, "retained limit must advance");
        limit = next;
    }
    assert!(
        found_description,
        "description was not refused at its own admission"
    );
    let admitted = layer_metadata_with_description(" description ");
    assert_eq!(
        admitted.layers[0].description.as_deref(),
        Some("description")
    );
}

#[test]
fn layer_description_uses_opennurbs_trim_set() {
    for (description, expected) in [
        ("\u{1680}description\u{1680}", "\u{1680}description\u{1680}"),
        ("\u{205f}description\u{205f}", "\u{205f}description\u{205f}"),
        ("\u{3000}description\u{3000}", "\u{3000}description\u{3000}"),
        (" description ", "description"),
    ] {
        let metadata = layer_metadata_with_description(description);
        assert_eq!(metadata.layers.len(), 1);
        assert_eq!(metadata.layers[0].description.as_deref(), Some(expected));
        assert_eq!(metadata.layers[0].extension_items, vec![37]);
    }
}

#[test]
fn layer_out_of_order_id_leaves_value_at_boundary() {
    // Item 33 follows item 34. The source cascade consumes the ID and closes
    // the class-data scan; the following byte is not a linetype payload.
    let metadata = layer_metadata_with_extension(&[34, 1, 33, 0xaa]);
    assert_eq!(metadata.layers.len(), 1);
    assert_eq!(metadata.layers[0].extension_items, vec![34]);
    assert!(metadata.layers[0].embedded_linetype.is_none());
}
