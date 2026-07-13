// SPDX-License-Identifier: Apache-2.0
//! Rhino document properties, selectors, previews, and setting identities.

use cadmpeg_ir::document::CadIr;
use serde::Serialize;

use crate::container::Scan;

#[derive(Debug, Serialize)]
struct RevisionRecord {
    id: String,
    source_offset: u64,
    created_by: String,
    created_utc_fields: [i32; 8],
    last_edited_by: String,
    last_edited_utc_fields: [i32; 8],
    revision_count: i32,
}

#[derive(Debug, Serialize)]
struct NotesRecord {
    id: String,
    source_offset: u64,
    html: bool,
    text: String,
    visible: bool,
    window_rectangle: [i32; 4],
    locked: bool,
}

#[derive(Debug, Serialize)]
struct ApplicationRecord {
    id: String,
    source_offset: u64,
    name: String,
    url: String,
    details: String,
}

#[derive(Debug, Serialize)]
struct DocumentSettingsRecord {
    id: String,
    writer_version: Option<i64>,
    archive_file_name: Option<String>,
    model_url: Option<String>,
    current_layer_index: Option<i64>,
    current_material_index: Option<i32>,
    current_material_source: Option<i32>,
    current_color: Option<[u8; 4]>,
    current_color_source: Option<i32>,
    current_wire_density: Option<i64>,
    current_font_index: Option<i64>,
    current_dimension_style_index: Option<i64>,
}

#[derive(Debug, Serialize)]
struct PreviewRecord {
    id: String,
    source_offset: u64,
    byte_len: u64,
    compressed: bool,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct SettingRecord {
    id: String,
    source_offset: u64,
    byte_len: u64,
    typecode: String,
    sha256: String,
}

/// Installs complete typed document-level metadata and named setting records.
pub(crate) fn install(scan: &Scan, ir: &mut CadIr) {
    let properties = &scan.metadata.properties;
    let revisions = properties
        .revision_history
        .iter()
        .map(|value| RevisionRecord {
            id: "rhino:document:revision#current".to_string(),
            source_offset: value.source.range.start as u64,
            created_by: value.created_by.clone(),
            created_utc_fields: value.created.fields,
            last_edited_by: value.last_edited_by.clone(),
            last_edited_utc_fields: value.last_edited.fields,
            revision_count: value.revision_count,
        })
        .collect::<Vec<_>>();
    let notes = properties
        .notes
        .iter()
        .map(|value| NotesRecord {
            id: "rhino:document:notes#current".to_string(),
            source_offset: value.source.range.start as u64,
            html: value.html != 0,
            text: value.text.clone(),
            visible: value.visible != 0,
            window_rectangle: value.rectangle,
            locked: value.locked,
        })
        .collect::<Vec<_>>();
    let applications = properties
        .application
        .iter()
        .map(|value| ApplicationRecord {
            id: "rhino:document:application#writer".to_string(),
            source_offset: value.source.range.start as u64,
            name: value.name.clone(),
            url: value.url.clone(),
            details: value.details.clone(),
        })
        .collect::<Vec<_>>();
    let settings = &scan.metadata.settings;
    let document_settings = [DocumentSettingsRecord {
        id: "rhino:document:settings#current".to_string(),
        writer_version: properties.writer_version,
        archive_file_name: properties.as_file_name.clone(),
        model_url: settings.model_url.clone(),
        current_layer_index: settings.current_layer,
        current_material_index: settings.current_material,
        current_material_source: settings.current_material_source,
        current_color: settings.current_color,
        current_color_source: settings.current_color_source,
        current_wire_density: settings.current_wire_density,
        current_font_index: settings.current_font,
        current_dimension_style_index: settings.current_dimstyle,
    }];
    let previews = properties
        .previews
        .iter()
        .enumerate()
        .map(|(index, value)| PreviewRecord {
            id: format!("rhino:document:preview#{index:04}"),
            source_offset: value.source.range.start as u64,
            byte_len: value.source.range.len() as u64,
            compressed: value.compressed,
            sha256: cadmpeg_ir::hash::sha256_hex(&scan.data[value.source.range.clone()]),
        })
        .collect::<Vec<_>>();
    let setting_records = settings
        .unsupported
        .iter()
        .enumerate()
        .map(|(index, value)| SettingRecord {
            id: format!("rhino:document:setting#{index:04}"),
            source_offset: value.source.range.start as u64,
            byte_len: value.source.range.len() as u64,
            typecode: format!("{:#010x}", value.typecode),
            sha256: cadmpeg_ir::hash::sha256_hex(&scan.data[value.source.range.clone()]),
        })
        .collect::<Vec<_>>();
    let namespace = ir.native.namespace_mut("rhino");
    namespace.version = namespace.version.max(2);
    namespace
        .set_arena("revisions", &revisions)
        .expect("Rhino revisions serialize");
    namespace
        .set_arena("document_notes", &notes)
        .expect("Rhino notes serialize");
    namespace
        .set_arena("applications", &applications)
        .expect("Rhino applications serialize");
    namespace
        .set_arena("document_settings", &document_settings)
        .expect("Rhino settings serialize");
    namespace
        .set_arena("previews", &previews)
        .expect("Rhino previews serialize");
    namespace
        .set_arena("setting_records", &setting_records)
        .expect("Rhino setting records serialize");
}
