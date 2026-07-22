// SPDX-License-Identifier: Apache-2.0
//! Declarative catalogue of the native record families.
//!
//! One [`CatalogueRow`] per model field (179 total). Each row names the `nx`
//! namespace arena the family serializes into, and — for families that also emit
//! source annotations — the tag, exactness, and a `note` fn. Row order is the
//! observable annotation-emission order for the note-bearing rows;
//! [`NOTE_GROUP_A_END`] / [`NOTE_GROUP_B_END`] mark the semantic-island split
//! that [`super::attach`] walks. Arena serialization order is not observable
//! (arenas live in a `BTreeMap`), so the non-noting tail rows follow the legacy
//! arena-pass order purely for readability.
//!
//! Whether a family notes into the shared `nx:container` stream or a per-record
//! `nx:s{ordinal}` stream is encoded in its `note` fn, not a row field.
//!
//! Per the IR-write firewall this module names `cadmpeg_ir` boundary types
//! (`AnnotationBuilder`, `NativeNamespace`, `Exactness`, `NativeConvertError`)
//! and calls the annotation/arena mutation surface from the row fns; the five
//! domain modules and `model.rs` carry no `cadmpeg_ir` reference.

use cadmpeg_ir::{AnnotationBuilder, Exactness, NativeConvertError, NativeNamespace};

use super::model::NativeModel;

/// One native record family: its arena, note metadata, and the fns that
/// serialize and (optionally) annotate it.
pub(crate) struct CatalogueRow {
    /// The `nx` namespace arena name.
    pub(crate) arena: &'static str,
    /// Annotation tag for standard note rows; `None` for custom-note and
    /// arena-only rows.
    pub(crate) tag: Option<&'static str>,
    /// Entity exactness for the family's standard notes.
    pub(crate) exactness: Exactness,
    /// Emits this family's annotations, or `None` for arena-only families and
    /// families whose notes a semantic island emits.
    pub(crate) note: Option<fn(&NativeModel, &CatalogueRow, &mut AnnotationBuilder)>,
    /// Serializes this family into its arena when non-empty.
    pub(crate) emit:
        fn(&NativeModel, &CatalogueRow, &mut NativeNamespace) -> Result<(), NativeConvertError>,
    /// Record count for this family, feeding the catalogue-derived emptiness
    /// fold ([`NativeModel::is_empty`]) and inspect counts.
    pub(crate) len: fn(&NativeModel) -> usize,
    /// Whether an empty family contributes to [`NativeModel::is_empty`]. 133 of
    /// the 179 families count; the 46 that do not are transcribed verbatim from
    /// the legacy hand-written all-empty guard, which omitted them. The
    /// exclusions look like oversights (25 of the 26 `display_jt` families are
    /// excluded, for instance) but are frozen observable behavior: flipping any
    /// one changes whether a part is treated as empty and therefore its output.
    pub(crate) counts_toward_emptiness: bool,
}

/// Index one past the last group-A note row. [`super::attach`] emits notes for
/// `CATALOGUE[..NOTE_GROUP_A_END]`, then the interleaved semantic islands, then
/// the group-B notes in `CATALOGUE[NOTE_GROUP_A_END..NOTE_GROUP_B_END]`.
pub(crate) const NOTE_GROUP_A_END: usize = 80;
/// Index one past the last group-B note row; rows beyond it are arena-only or
/// island-noted (`part_attributes`, `configurations`).
pub(crate) const NOTE_GROUP_B_END: usize = 83;

fn note_display_jt_display_jt_indices(
    m: &NativeModel,
    _catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let annotation_stream = a.stream("nx:container");
    for index in &m.display_jt.display_jt_indices {
        a.note(&index.id, annotation_stream, index.source_offset)
            .tag("DISPLAY_JT_INDEX");
        a.exactness(&index.id, Exactness::ByteExact);
        for row in &index.rows {
            a.note(&row.id, annotation_stream, row.source_offset)
                .tag("DISPLAY_JT_INDEX_ROW");
            a.exactness(&row.id, Exactness::ByteExact);
        }
    }
}

fn note_display_jt_display_jt_documents(
    m: &NativeModel,
    _catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let annotation_stream = a.stream("nx:container");
    for document in &m.display_jt.display_jt_documents {
        a.note(&document.id, annotation_stream, document.source_offset)
            .tag("DISPLAY_JT_DOCUMENT");
        a.exactness(&document.id, Exactness::ByteExact);
        for entry in &document.toc_entries {
            a.note(&entry.id, annotation_stream, entry.source_offset)
                .tag("DISPLAY_JT_TOC_ENTRY");
            a.exactness(&entry.id, Exactness::ByteExact);
        }
    }
}

fn note_display_jt_display_jt_segments(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for segment in &m.display_jt.display_jt_segments {
        a.note(&segment.id, stream, segment.source_offset).tag(tag);
        a.exactness(&segment.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_shape_lod_elements(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for element in &m.display_jt.display_jt_shape_lod_elements {
        a.note(&element.id, stream, element.source_offset).tag(tag);
        a.exactness(&element.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_tri_strip_lod_headers(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for header in &m.display_jt.display_jt_tri_strip_lod_headers {
        a.note(&header.id, stream, header.source_offset).tag(tag);
        a.exactness(&header.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_initial_face_degree_symbols(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for symbols in &m.display_jt.display_jt_initial_face_degree_symbols {
        a.note(&symbols.id, stream, symbols.source_offset).tag(tag);
        a.exactness(&symbols.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_topology_packet_sequences(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for sequence in &m.display_jt.display_jt_topology_packet_sequences {
        a.note(&sequence.id, stream, sequence.source_offset)
            .tag(tag);
        a.exactness(&sequence.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_vertex_records_headers(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for header in &m.display_jt.display_jt_vertex_records_headers {
        a.note(&header.id, stream, header.source_offset).tag(tag);
        a.exactness(&header.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_coordinate_array_headers(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for header in &m.display_jt.display_jt_coordinate_array_headers {
        a.note(&header.id, stream, header.source_offset).tag(tag);
        a.exactness(&header.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_vertex_coordinates(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for coordinates in &m.display_jt.display_jt_vertex_coordinates {
        a.note(&coordinates.id, stream, coordinates.source_offset)
            .tag(tag);
        a.exactness(&coordinates.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_vertex_normals(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for normals in &m.display_jt.display_jt_vertex_normals {
        a.note(&normals.id, stream, normals.source_offset).tag(tag);
        a.exactness(&normals.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_vertex_colors(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for colors in &m.display_jt.display_jt_vertex_colors {
        a.note(&colors.id, stream, colors.source_offset).tag(tag);
        a.exactness(&colors.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_vertex_texture_coordinates(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for texture_coordinates in &m.display_jt.display_jt_vertex_texture_coordinates {
        a.note(
            &texture_coordinates.id,
            stream,
            texture_coordinates.source_offset,
        )
        .tag(tag);
        a.exactness(&texture_coordinates.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_vertex_flags(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for flags in &m.display_jt.display_jt_vertex_flags {
        a.note(&flags.id, stream, flags.source_offset).tag(tag);
        a.exactness(&flags.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_geometric_transform_attributes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for transform in &m.display_jt.display_jt_geometric_transform_attributes {
        a.note(&transform.id, stream, transform.source_offset)
            .tag(tag);
        a.exactness(&transform.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_polygon_meshes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for mesh in &m.display_jt.display_jt_polygon_meshes {
        a.note(&mesh.id, stream, mesh.source_offset).tag(tag);
        a.exactness(&mesh.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_compressed_element_sequences(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for sequence in &m.display_jt.display_jt_compressed_element_sequences {
        a.note(&sequence.id, stream, sequence.source_offset)
            .tag(tag);
        a.exactness(&sequence.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_compressed_elements(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for element in &m.display_jt.display_jt_compressed_elements {
        a.note(&element.id, stream, element.source_offset).tag(tag);
        a.exactness(&element.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_string_property_atoms(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for atom in &m.display_jt.display_jt_string_property_atoms {
        a.note(&atom.id, stream, atom.source_offset).tag(tag);
        a.exactness(&atom.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_shape_lod_bindings(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for binding in &m.display_jt.display_jt_shape_lod_bindings {
        a.note(&binding.id, stream, binding.source_offset).tag(tag);
        a.exactness(&binding.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_base_node_data(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for node in &m.display_jt.display_jt_base_node_data {
        a.note(&node.id, stream, node.source_offset).tag(tag);
        a.exactness(&node.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_group_node_data(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for node in &m.display_jt.display_jt_group_node_data {
        a.note(&node.id, stream, node.source_offset).tag(tag);
        a.exactness(&node.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_instance_nodes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for node in &m.display_jt.display_jt_instance_nodes {
        a.note(&node.id, stream, node.source_offset).tag(tag);
        a.exactness(&node.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_partition_nodes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for node in &m.display_jt.display_jt_partition_nodes {
        a.note(&node.id, stream, node.source_offset).tag(tag);
        a.exactness(&node.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_range_lod_nodes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for node in &m.display_jt.display_jt_range_lod_nodes {
        a.note(&node.id, stream, node.source_offset).tag(tag);
        a.exactness(&node.id, catalogue_row.exactness);
    }
}

fn note_display_jt_display_jt_tri_strip_shape_nodes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for node in &m.display_jt.display_jt_tri_strip_shape_nodes {
        a.note(&node.id, stream, node.source_offset).tag(tag);
        a.exactness(&node.id, catalogue_row.exactness);
    }
}

fn note_segments_segment_index_rows(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for row in &m.segments.segment_index_rows {
        a.note(&row.id, stream, row.source_offset).tag(tag);
        a.exactness(&row.id, catalogue_row.exactness);
    }
}

fn note_segments_segment_stream_links(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for link in &m.segments.segment_stream_links {
        a.note(&link.id, stream, link.source_offset).tag(tag);
        a.exactness(&link.id, catalogue_row.exactness);
    }
}

fn note_segments_segment_body_bindings(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for binding in &m.segments.segment_body_bindings {
        a.note(&binding.id, stream, binding.source_offset).tag(tag);
        a.exactness(&binding.id, catalogue_row.exactness);
    }
}

fn note_segments_segment_body_lineage_statuses(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for status in &m.segments.segment_body_lineage_statuses {
        a.note(&status.id, stream, status.source_offset).tag(tag);
        a.exactness(&status.id, catalogue_row.exactness);
    }
}

fn note_parasolid_parasolid_blend_surface_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    for record in &m.parasolid.parasolid_blend_surface_records {
        let stream = a.stream(format!("nx:s{}", record.stream_ordinal));
        a.note(&record.id, stream, record.inflated_offset).tag(tag);
        a.exactness(&record.id, catalogue_row.exactness);
    }
}

fn note_parasolid_parasolid_blend_bound_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    for record in &m.parasolid.parasolid_blend_bound_records {
        let stream = a.stream(format!("nx:s{}", record.stream_ordinal));
        a.note(&record.id, stream, record.inflated_offset).tag(tag);
        a.exactness(&record.id, catalogue_row.exactness);
    }
}

fn note_parasolid_parasolid_offset_surface_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    for record in &m.parasolid.parasolid_offset_surface_records {
        let stream = a.stream(format!("nx:s{}", record.stream_ordinal));
        a.note(&record.id, stream, record.inflated_offset).tag(tag);
        a.exactness(&record.id, catalogue_row.exactness);
    }
}

fn note_parasolid_parasolid_trimmed_curve_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    for record in &m.parasolid.parasolid_trimmed_curve_records {
        let stream = a.stream(format!("nx:s{}", record.stream_ordinal));
        a.note(&record.id, stream, record.inflated_offset).tag(tag);
        a.exactness(&record.id, catalogue_row.exactness);
    }
}

fn note_parasolid_parasolid_surface_curve_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    for record in &m.parasolid.parasolid_surface_curve_records {
        let stream = a.stream(format!("nx:s{}", record.stream_ordinal));
        a.note(&record.id, stream, record.inflated_offset).tag(tag);
        a.exactness(&record.id, catalogue_row.exactness);
    }
}

fn note_parasolid_parasolid_intersection_records(
    m: &NativeModel,
    _catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    for record in &m.parasolid.parasolid_intersection_records {
        let source_stream = a.stream(format!("nx:s{}", record.stream_ordinal));
        a.note(&record.id, source_stream, record.inflated_offset)
            .tag(if record.delta_twin {
                "INTERSECTION_DATA"
            } else {
                "INTERSECTION"
            });
        a.exactness(&record.id, Exactness::ByteExact);
    }
}

fn note_parasolid_parasolid_term_use_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    for record in &m.parasolid.parasolid_term_use_records {
        let stream = a.stream(format!("nx:s{}", record.stream_ordinal));
        a.note(&record.id, stream, record.inflated_offset).tag(tag);
        a.exactness(&record.id, catalogue_row.exactness);
    }
}

fn note_parasolid_parasolid_support_uv_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    for record in &m.parasolid.parasolid_support_uv_records {
        let stream = a.stream(format!("nx:s{}", record.stream_ordinal));
        a.note(&record.id, stream, record.inflated_offset).tag(tag);
        a.exactness(&record.id, catalogue_row.exactness);
    }
}

fn note_parasolid_parasolid_chart_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    for record in &m.parasolid.parasolid_chart_records {
        let stream = a.stream(format!("nx:s{}", record.stream_ordinal));
        a.note(&record.id, stream, record.inflated_offset).tag(tag);
        a.exactness(&record.id, catalogue_row.exactness);
    }
}

fn note_parasolid_parasolid_attribute_definitions(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    for definition in &m.parasolid.parasolid_attribute_definitions {
        let stream = a.stream(format!("nx:s{}", definition.stream_ordinal));
        a.note(&definition.id, stream, definition.inflated_offset)
            .tag(tag);
        a.exactness(&definition.id, catalogue_row.exactness);
    }
}

fn note_parasolid_parasolid_entity_51_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    for record in &m.parasolid.parasolid_entity_51_records {
        let stream = a.stream(format!("nx:s{}", record.stream_ordinal));
        a.note(&record.id, stream, record.inflated_offset).tag(tag);
        a.exactness(&record.id, catalogue_row.exactness);
    }
}

fn note_parasolid_parasolid_entity_52_integer_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    for record in &m.parasolid.parasolid_entity_52_integer_records {
        let stream = a.stream(format!("nx:s{}", record.stream_ordinal));
        a.note(&record.id, stream, record.inflated_offset).tag(tag);
        a.exactness(&record.id, catalogue_row.exactness);
    }
}

fn note_parasolid_parasolid_entity_53_double_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    for record in &m.parasolid.parasolid_entity_53_double_records {
        let stream = a.stream(format!("nx:s{}", record.stream_ordinal));
        a.note(&record.id, stream, record.inflated_offset).tag(tag);
        a.exactness(&record.id, catalogue_row.exactness);
    }
}

fn note_parasolid_parasolid_entity_54_string_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    for record in &m.parasolid.parasolid_entity_54_string_records {
        let stream = a.stream(format!("nx:s{}", record.stream_ordinal));
        a.note(&record.id, stream, record.inflated_offset).tag(tag);
        a.exactness(&record.id, catalogue_row.exactness);
    }
}

fn note_parasolid_parasolid_entity_51_string_uses(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    for block_use in &m.parasolid.parasolid_entity_51_string_uses {
        let stream = a.stream(format!("nx:s{}", block_use.stream_ordinal));
        a.note(&block_use.id, stream, block_use.inflated_offset)
            .tag(tag);
        a.exactness(&block_use.id, catalogue_row.exactness);
    }
}

fn note_parasolid_parasolid_entity_51_numeric_uses(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    for value_use in &m.parasolid.parasolid_entity_51_numeric_uses {
        let stream = a.stream(format!("nx:s{}", value_use.stream_ordinal));
        a.note(&value_use.id, stream, value_use.inflated_offset)
            .tag(tag);
        a.exactness(&value_use.id, catalogue_row.exactness);
    }
}

fn note_parasolid_parasolid_attribute_class_uses(
    m: &NativeModel,
    _catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    for class_use in &m.parasolid.parasolid_attribute_class_uses {
        let entity = m
            .parasolid
            .parasolid_entity_51_records
            .iter()
            .find(|entity| entity.id == class_use.entity_51_record)
            .expect("class use owns a type-81 entity");
        let source_stream = a.stream(format!("nx:s{}", class_use.stream_ordinal));
        a.note(&class_use.id, source_stream, entity.inflated_offset)
            .tag("ATTRIBUTE_CLASS_USE");
        a.exactness(&class_use.id, Exactness::Derived);
    }
}

fn note_parasolid_parasolid_topology_attribute_list_references(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    for reference in &m.parasolid.parasolid_topology_attribute_list_references {
        let stream = a.stream(format!("nx:s{}", reference.stream_ordinal));
        a.note(&reference.id, stream, reference.inflated_offset)
            .tag(tag);
        a.exactness(&reference.id, catalogue_row.exactness);
    }
}

fn note_parasolid_parasolid_topology_attribute_class_uses(
    m: &NativeModel,
    _catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    for class_use in &m.parasolid.parasolid_topology_attribute_class_uses {
        let reference = m
            .parasolid
            .parasolid_topology_attribute_list_references
            .iter()
            .find(|reference| reference.id == class_use.topology_attribute_reference)
            .expect("class use owns a topology attribute reference");
        let source_stream = a.stream(format!("nx:s{}", reference.stream_ordinal));
        let entity = m
            .parasolid
            .parasolid_entity_51_records
            .iter()
            .find(|entity| entity.id == class_use.entity_51_record)
            .expect("class use owns a type-81 entity");
        a.note(&class_use.id, source_stream, entity.inflated_offset)
            .tag("TOPOLOGY_ATTRIBUTE_CLASS_USE");
        a.exactness(&class_use.id, Exactness::Derived);
    }
}

fn note_features_data_block_object_frames(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for frame in &m.features.data_block_object_frames {
        a.note(&frame.id, stream, frame.source_offset).tag(tag);
        a.exactness(&frame.id, catalogue_row.exactness);
    }
}

fn note_features_offset_store_named_points(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for point in &m.features.offset_store_named_points {
        a.note(&point.id, stream, point.source_offset).tag(tag);
        a.exactness(&point.id, catalogue_row.exactness);
    }
}

fn note_features_feature_sketch_named_point_block_uses(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for block_use in &m.features.feature_sketch_named_point_block_uses {
        a.note(&block_use.id, stream, block_use.source_offset)
            .tag(tag);
        a.exactness(&block_use.id, catalogue_row.exactness);
    }
}

fn note_features_feature_sketch_preceding_named_point_uses(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for point_use in &m.features.feature_sketch_preceding_named_point_uses {
        a.note(&point_use.id, stream, point_use.source_offset)
            .tag(tag);
        a.exactness(&point_use.id, catalogue_row.exactness);
    }
}

fn note_features_feature_sketch_point_uses(
    m: &NativeModel,
    _catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let annotation_stream = a.stream("nx:container");
    for point_use in &m.features.feature_sketch_point_uses {
        a.note(
            &point_use.id,
            annotation_stream,
            point_use.source_offsets[0],
        )
        .tag("SKETCH_POINT_USE");
        a.exactness(&point_use.id, Exactness::Derived);
    }
}

fn note_features_feature_sketch_datum_csys_dependencies(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for dependency in &m.features.feature_sketch_datum_csys_dependencies {
        a.note(&dependency.id, stream, dependency.source_offset)
            .tag(tag);
        a.exactness(&dependency.id, catalogue_row.exactness);
    }
}

fn note_features_feature_input_block_identity_groups(
    m: &NativeModel,
    _catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let annotation_stream = a.stream("nx:container");
    for group in &m.features.feature_input_block_identity_groups {
        a.note(&group.id, annotation_stream, group.source_offsets[0])
            .tag("FEATURE_INPUT_BLOCK_IDENTITY_GROUP");
        a.exactness(&group.id, Exactness::ByteExact);
    }
}

fn note_om_data_block_abr_reference_lanes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for lane in &m.om.data_block_abr_reference_lanes {
        a.note(&lane.id, stream, lane.source_offset).tag(tag);
        a.exactness(&lane.id, catalogue_row.exactness);
    }
}

fn note_segments_segment_om_links(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for link in &m.segments.segment_om_links {
        a.note(&link.id, stream, link.source_offset).tag(tag);
        a.exactness(&link.id, catalogue_row.exactness);
    }
}

fn note_om_om_record_areas(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for area in &m.om.om_record_areas {
        a.note(&area.id, stream, area.source_offset).tag(tag);
        a.exactness(&area.id, catalogue_row.exactness);
    }
}

fn note_features_feature_operation_labels(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for label in &m.features.feature_operation_labels {
        a.note(&label.id, stream, label.source_offset).tag(tag);
        a.exactness(&label.id, catalogue_row.exactness);
    }
}

fn note_features_feature_sketch_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for sketch in &m.features.feature_sketch_records {
        a.note(&sketch.id, stream, sketch.source_offset).tag(tag);
        a.exactness(&sketch.id, catalogue_row.exactness);
    }
}

fn note_features_feature_sketch_payload_fixed_pairs(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for pair in &m.features.feature_sketch_payload_fixed_pairs {
        a.note(&pair.id, stream, pair.source_offset).tag(tag);
        a.exactness(&pair.id, catalogue_row.exactness);
    }
}

fn note_features_feature_sketch_fixed_points(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for point in &m.features.feature_sketch_fixed_points {
        a.note(&point.id, stream, point.source_offset).tag(tag);
        a.exactness(&point.id, catalogue_row.exactness);
    }
}

fn note_features_feature_operation_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for record in &m.features.feature_operation_records {
        a.note(&record.id, stream, record.source_offset).tag(tag);
        a.exactness(&record.id, catalogue_row.exactness);
    }
}

fn note_features_feature_payload_strings(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for value in &m.features.feature_payload_strings {
        a.note(&value.id, stream, value.source_offset).tag(tag);
        a.exactness(&value.id, catalogue_row.exactness);
    }
}

fn note_features_feature_body_references(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for reference in &m.features.feature_body_references {
        a.note(&reference.id, stream, reference.source_offset)
            .tag(tag);
        a.exactness(&reference.id, catalogue_row.exactness);
    }
}

fn note_features_feature_body_reference_occurrences(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for reference in &m.features.feature_body_reference_occurrences {
        a.note(&reference.id, stream, reference.source_offset)
            .tag(tag);
        a.exactness(&reference.id, catalogue_row.exactness);
    }
}

fn note_features_feature_input_blocks(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for input in &m.features.feature_input_blocks {
        a.note(&input.id, stream, input.source_offset).tag(tag);
        a.exactness(&input.id, catalogue_row.exactness);
    }
}

fn note_features_feature_boolean_operations(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for operation in &m.features.feature_boolean_operations {
        a.note(&operation.id, stream, operation.source_offset)
            .tag(tag);
        a.exactness(&operation.id, catalogue_row.exactness);
    }
}

fn note_om_expression_declarations(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for declaration in &m.om.expression_declarations {
        a.note(&declaration.id, stream, declaration.source_offset)
            .tag(tag);
        a.exactness(&declaration.id, catalogue_row.exactness);
    }
}

fn note_om_data_block_control_values(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for value in &m.om.data_block_control_values {
        a.note(&value.id, stream, value.source_offset).tag(tag);
        a.exactness(&value.id, catalogue_row.exactness);
    }
}

fn note_om_data_block_control_class_references(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for reference in &m.om.data_block_control_class_references {
        a.note(&reference.id, stream, reference.source_offset)
            .tag(tag);
        a.exactness(&reference.id, catalogue_row.exactness);
    }
}

fn note_om_data_block_control_index_values(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for value in &m.om.data_block_control_index_values {
        a.note(&value.id, stream, value.source_offset).tag(tag);
        a.exactness(&value.id, catalogue_row.exactness);
    }
}

fn note_om_data_block_control_references(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for reference in &m.om.data_block_control_references {
        a.note(&reference.id, stream, reference.source_offset)
            .tag(tag);
        a.exactness(&reference.id, catalogue_row.exactness);
    }
}

fn note_om_data_block_control_handle_pairs(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for pair in &m.om.data_block_control_handle_pairs {
        a.note(&pair.id, stream, pair.source_offset).tag(tag);
        a.exactness(&pair.id, catalogue_row.exactness);
    }
}

fn note_om_data_block_references(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for reference in &m.om.data_block_references {
        a.note(&reference.id, stream, reference.source_offset)
            .tag(tag);
        a.exactness(&reference.id, catalogue_row.exactness);
    }
}

fn note_features_feature_parameter_bindings(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for binding in &m.features.feature_parameter_bindings {
        a.note(&binding.id, stream, binding.source_offset).tag(tag);
        a.exactness(&binding.id, catalogue_row.exactness);
    }
}

fn note_features_feature_parameter_uses(
    m: &NativeModel,
    _catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let annotation_stream = a.stream("nx:container");
    for parameter_use in &m.features.feature_parameter_uses {
        a.note(
            &parameter_use.id,
            annotation_stream,
            parameter_use.source_offsets[0],
        )
        .tag("FEATURE_PARAMETER_USE");
        a.exactness(&parameter_use.id, Exactness::Derived);
    }
}

fn note_om_store_headers(m: &NativeModel, catalogue_row: &CatalogueRow, a: &mut AnnotationBuilder) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for header in &m.om.store_headers {
        a.note(&header.id, stream, header.source_offset).tag(tag);
        a.exactness(&header.id, catalogue_row.exactness);
    }
}

fn note_om_external_references(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for reference in &m.om.external_references {
        a.note(&reference.id, stream, reference.source_offset)
            .tag(tag);
        a.exactness(&reference.id, catalogue_row.exactness);
    }
}

fn note_om_external_reference_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for record in &m.om.external_reference_records {
        a.note(&record.id, stream, record.source_offset).tag(tag);
        a.exactness(&record.id, catalogue_row.exactness);
    }
}

fn note_om_material_texture_assets(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for asset in &m.om.material_texture_assets {
        a.note(&asset.id, stream, asset.source_offset).tag(tag);
        a.exactness(&asset.id, catalogue_row.exactness);
    }
}

fn note_om_material_texture_catalog_entries(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for entry in &m.om.material_texture_catalog_entries {
        a.note(&entry.id, stream, entry.source_offset).tag(tag);
        a.exactness(&entry.id, catalogue_row.exactness);
    }
}

fn emit_segments_segment_index_rows(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.segments.segment_index_rows.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.segments.segment_index_rows)?;
    }
    Ok(())
}

fn emit_segments_segment_stream_links(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.segments.segment_stream_links.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.segments.segment_stream_links)?;
    }
    Ok(())
}

fn emit_segments_segment_body_bindings(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.segments.segment_body_bindings.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.segments.segment_body_bindings)?;
    }
    Ok(())
}

fn emit_features_feature_body_segment_uses(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_body_segment_uses.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.feature_body_segment_uses)?;
    }
    Ok(())
}

fn emit_segments_segment_body_lineage_statuses(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.segments.segment_body_lineage_statuses.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.segments.segment_body_lineage_statuses,
        )?;
    }
    Ok(())
}

fn emit_parasolid_parasolid_blend_surface_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.parasolid.parasolid_blend_surface_records.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.parasolid.parasolid_blend_surface_records,
        )?;
    }
    Ok(())
}

fn emit_parasolid_parasolid_blend_bound_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.parasolid.parasolid_blend_bound_records.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.parasolid.parasolid_blend_bound_records,
        )?;
    }
    Ok(())
}

fn emit_parasolid_parasolid_offset_surface_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.parasolid.parasolid_offset_surface_records.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.parasolid.parasolid_offset_surface_records,
        )?;
    }
    Ok(())
}

fn emit_parasolid_parasolid_trimmed_curve_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.parasolid.parasolid_trimmed_curve_records.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.parasolid.parasolid_trimmed_curve_records,
        )?;
    }
    Ok(())
}

fn emit_parasolid_parasolid_surface_curve_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.parasolid.parasolid_surface_curve_records.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.parasolid.parasolid_surface_curve_records,
        )?;
    }
    Ok(())
}

fn emit_parasolid_parasolid_intersection_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.parasolid.parasolid_intersection_records.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.parasolid.parasolid_intersection_records,
        )?;
    }
    Ok(())
}

fn emit_parasolid_parasolid_term_use_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.parasolid.parasolid_term_use_records.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.parasolid.parasolid_term_use_records)?;
    }
    Ok(())
}

fn emit_parasolid_parasolid_support_uv_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.parasolid.parasolid_support_uv_records.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.parasolid.parasolid_support_uv_records,
        )?;
    }
    Ok(())
}

fn emit_parasolid_parasolid_chart_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.parasolid.parasolid_chart_records.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.parasolid.parasolid_chart_records)?;
    }
    Ok(())
}

fn emit_parasolid_parasolid_attribute_definitions(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.parasolid.parasolid_attribute_definitions.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.parasolid.parasolid_attribute_definitions,
        )?;
    }
    Ok(())
}

fn emit_parasolid_parasolid_entity_51_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.parasolid.parasolid_entity_51_records.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.parasolid.parasolid_entity_51_records,
        )?;
    }
    Ok(())
}

fn emit_parasolid_parasolid_entity_52_integer_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.parasolid.parasolid_entity_52_integer_records.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.parasolid.parasolid_entity_52_integer_records,
        )?;
    }
    Ok(())
}

fn emit_parasolid_parasolid_entity_53_double_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.parasolid.parasolid_entity_53_double_records.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.parasolid.parasolid_entity_53_double_records,
        )?;
    }
    Ok(())
}

fn emit_parasolid_parasolid_entity_54_string_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.parasolid.parasolid_entity_54_string_records.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.parasolid.parasolid_entity_54_string_records,
        )?;
    }
    Ok(())
}

fn emit_parasolid_parasolid_entity_51_string_uses(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.parasolid.parasolid_entity_51_string_uses.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.parasolid.parasolid_entity_51_string_uses,
        )?;
    }
    Ok(())
}

fn emit_parasolid_parasolid_entity_51_numeric_uses(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.parasolid.parasolid_entity_51_numeric_uses.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.parasolid.parasolid_entity_51_numeric_uses,
        )?;
    }
    Ok(())
}

fn emit_parasolid_parasolid_attribute_class_uses(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.parasolid.parasolid_attribute_class_uses.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.parasolid.parasolid_attribute_class_uses,
        )?;
    }
    Ok(())
}

fn emit_parasolid_parasolid_topology_attribute_list_references(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .parasolid
        .parasolid_topology_attribute_list_references
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.parasolid.parasolid_topology_attribute_list_references,
        )?;
    }
    Ok(())
}

fn emit_parasolid_parasolid_topology_attribute_class_uses(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .parasolid
        .parasolid_topology_attribute_class_uses
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.parasolid.parasolid_topology_attribute_class_uses,
        )?;
    }
    Ok(())
}

fn emit_segments_segment_om_links(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.segments.segment_om_links.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.segments.segment_om_links)?;
    }
    Ok(())
}

fn emit_om_om_record_areas(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.om_record_areas.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.om_record_areas)?;
    }
    Ok(())
}

fn emit_features_feature_operation_labels(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_operation_labels.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.feature_operation_labels)?;
    }
    Ok(())
}

fn emit_features_feature_operation_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_operation_records.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.feature_operation_records)?;
    }
    Ok(())
}

fn emit_features_feature_payload_strings(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_payload_strings.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.feature_payload_strings)?;
    }
    Ok(())
}

fn emit_features_feature_simple_hole_templates(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_simple_hole_templates.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_simple_hole_templates,
        )?;
    }
    Ok(())
}

fn emit_features_feature_simple_hole_repeated_scalar_lanes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .features
        .feature_simple_hole_repeated_scalar_lanes
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_simple_hole_repeated_scalar_lanes,
        )?;
    }
    Ok(())
}

fn emit_features_feature_simple_hole_repeated_scalar_lane_block_references(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .features
        .feature_simple_hole_repeated_scalar_lane_block_references
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.features
                .feature_simple_hole_repeated_scalar_lane_block_references,
        )?;
    }
    Ok(())
}

fn emit_features_feature_simple_hole_construction_groups(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .features
        .feature_simple_hole_construction_groups
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_simple_hole_construction_groups,
        )?;
    }
    Ok(())
}

fn emit_features_feature_body_references(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_body_references.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.feature_body_references)?;
    }
    Ok(())
}

fn emit_features_feature_body_reference_occurrences(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_body_reference_occurrences.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_body_reference_occurrences,
        )?;
    }
    Ok(())
}

fn emit_features_feature_input_blocks(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_input_blocks.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.feature_input_blocks)?;
    }
    Ok(())
}

fn emit_features_feature_input_block_identity_groups(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_input_block_identity_groups.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_input_block_identity_groups,
        )?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_indices(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.display_jt.display_jt_indices.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.display_jt.display_jt_indices)?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_documents(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.display_jt.display_jt_documents.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.display_jt.display_jt_documents)?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_segments(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.display_jt.display_jt_segments.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.display_jt.display_jt_segments)?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_shape_lod_elements(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.display_jt.display_jt_shape_lod_elements.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.display_jt.display_jt_shape_lod_elements,
        )?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_tri_strip_lod_headers(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.display_jt.display_jt_tri_strip_lod_headers.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.display_jt.display_jt_tri_strip_lod_headers,
        )?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_initial_face_degree_symbols(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .display_jt
        .display_jt_initial_face_degree_symbols
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.display_jt.display_jt_initial_face_degree_symbols,
        )?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_topology_packet_sequences(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.display_jt.display_jt_topology_packet_sequences.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.display_jt.display_jt_topology_packet_sequences,
        )?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_vertex_records_headers(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.display_jt.display_jt_vertex_records_headers.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.display_jt.display_jt_vertex_records_headers,
        )?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_coordinate_array_headers(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.display_jt.display_jt_coordinate_array_headers.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.display_jt.display_jt_coordinate_array_headers,
        )?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_vertex_coordinates(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.display_jt.display_jt_vertex_coordinates.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.display_jt.display_jt_vertex_coordinates,
        )?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_vertex_normals(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.display_jt.display_jt_vertex_normals.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.display_jt.display_jt_vertex_normals)?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_vertex_colors(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.display_jt.display_jt_vertex_colors.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.display_jt.display_jt_vertex_colors)?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_vertex_texture_coordinates(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .display_jt
        .display_jt_vertex_texture_coordinates
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.display_jt.display_jt_vertex_texture_coordinates,
        )?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_vertex_flags(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.display_jt.display_jt_vertex_flags.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.display_jt.display_jt_vertex_flags)?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_geometric_transform_attributes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .display_jt
        .display_jt_geometric_transform_attributes
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.display_jt.display_jt_geometric_transform_attributes,
        )?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_polygon_meshes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.display_jt.display_jt_polygon_meshes.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.display_jt.display_jt_polygon_meshes)?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_compressed_element_sequences(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .display_jt
        .display_jt_compressed_element_sequences
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.display_jt.display_jt_compressed_element_sequences,
        )?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_compressed_elements(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.display_jt.display_jt_compressed_elements.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.display_jt.display_jt_compressed_elements,
        )?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_string_property_atoms(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.display_jt.display_jt_string_property_atoms.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.display_jt.display_jt_string_property_atoms,
        )?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_shape_lod_bindings(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.display_jt.display_jt_shape_lod_bindings.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.display_jt.display_jt_shape_lod_bindings,
        )?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_base_node_data(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.display_jt.display_jt_base_node_data.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.display_jt.display_jt_base_node_data)?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_group_node_data(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.display_jt.display_jt_group_node_data.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.display_jt.display_jt_group_node_data,
        )?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_instance_nodes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.display_jt.display_jt_instance_nodes.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.display_jt.display_jt_instance_nodes)?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_partition_nodes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.display_jt.display_jt_partition_nodes.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.display_jt.display_jt_partition_nodes,
        )?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_range_lod_nodes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.display_jt.display_jt_range_lod_nodes.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.display_jt.display_jt_range_lod_nodes,
        )?;
    }
    Ok(())
}

fn emit_display_jt_display_jt_tri_strip_shape_nodes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.display_jt.display_jt_tri_strip_shape_nodes.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.display_jt.display_jt_tri_strip_shape_nodes,
        )?;
    }
    Ok(())
}

fn emit_features_feature_datum_csys_constructions(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_datum_csys_constructions.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_datum_csys_constructions,
        )?;
    }
    Ok(())
}

fn emit_features_feature_datum_csys_payloads(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_datum_csys_payloads.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.feature_datum_csys_payloads)?;
    }
    Ok(())
}

fn emit_features_feature_datum_csys_payload_scalar_pairs(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .features
        .feature_datum_csys_payload_scalar_pairs
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_datum_csys_payload_scalar_pairs,
        )?;
    }
    Ok(())
}

fn emit_features_feature_datum_csys_payload_fixed_pairs(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_datum_csys_payload_fixed_pairs.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_datum_csys_payload_fixed_pairs,
        )?;
    }
    Ok(())
}

fn emit_features_feature_datum_csys_payload_scalars(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_datum_csys_payload_scalars.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_datum_csys_payload_scalars,
        )?;
    }
    Ok(())
}

fn emit_features_feature_datum_csys_descriptors(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_datum_csys_descriptors.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_datum_csys_descriptors,
        )?;
    }
    Ok(())
}

fn emit_features_feature_datum_csys_block_uses(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_datum_csys_block_uses.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_datum_csys_block_uses,
        )?;
    }
    Ok(())
}

fn emit_features_feature_datum_plane_headers(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_datum_plane_headers.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.feature_datum_plane_headers)?;
    }
    Ok(())
}

fn emit_features_feature_datum_plane_block_uses(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_datum_plane_block_uses.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_datum_plane_block_uses,
        )?;
    }
    Ok(())
}

fn emit_features_feature_datum_plane_payloads(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_datum_plane_payloads.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_datum_plane_payloads,
        )?;
    }
    Ok(())
}

fn emit_features_feature_datum_plane_payload_scalar_pairs(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .features
        .feature_datum_plane_payload_scalar_pairs
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_datum_plane_payload_scalar_pairs,
        )?;
    }
    Ok(())
}

fn emit_features_feature_datum_plane_descriptors(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_datum_plane_descriptors.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_datum_plane_descriptors,
        )?;
    }
    Ok(())
}

fn emit_features_feature_datum_plane_csys_identity_uses(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_datum_plane_csys_identity_uses.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_datum_plane_csys_identity_uses,
        )?;
    }
    Ok(())
}

fn emit_features_feature_sketch_references(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_sketch_references.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.feature_sketch_references)?;
    }
    Ok(())
}

fn emit_features_feature_projected_curve_references(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_projected_curve_references.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_projected_curve_references,
        )?;
    }
    Ok(())
}

fn emit_features_feature_projected_curve_construction_payloads(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .features
        .feature_projected_curve_construction_payloads
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_projected_curve_construction_payloads,
        )?;
    }
    Ok(())
}

fn emit_features_feature_projected_curve_construction_strings(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .features
        .feature_projected_curve_construction_strings
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_projected_curve_construction_strings,
        )?;
    }
    Ok(())
}

fn emit_features_feature_pattern_references(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_pattern_references.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.feature_pattern_references)?;
    }
    Ok(())
}

fn emit_features_feature_pattern_construction_payloads(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_pattern_construction_payloads.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_pattern_construction_payloads,
        )?;
    }
    Ok(())
}

fn emit_features_feature_pattern_construction_strings(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_pattern_construction_strings.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_pattern_construction_strings,
        )?;
    }
    Ok(())
}

fn emit_features_feature_pattern_construction_fixed_lanes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .features
        .feature_pattern_construction_fixed_lanes
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_pattern_construction_fixed_lanes,
        )?;
    }
    Ok(())
}

fn emit_features_feature_pattern_transform_lanes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_pattern_transform_lanes.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_pattern_transform_lanes,
        )?;
    }
    Ok(())
}

fn emit_features_feature_point_construction_headers(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_point_construction_headers.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_point_construction_headers,
        )?;
    }
    Ok(())
}

fn emit_features_feature_point_construction_scalar_lanes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .features
        .feature_point_construction_scalar_lanes
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_point_construction_scalar_lanes,
        )?;
    }
    Ok(())
}

fn emit_features_feature_draft_construction_references(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_draft_construction_references.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_draft_construction_references,
        )?;
    }
    Ok(())
}

fn emit_features_feature_draft_construction_index_lanes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_draft_construction_index_lanes.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_draft_construction_index_lanes,
        )?;
    }
    Ok(())
}

fn emit_features_feature_draft_construction_payloads(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_draft_construction_payloads.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_draft_construction_payloads,
        )?;
    }
    Ok(())
}

fn emit_features_feature_draft_construction_graph_payloads(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .features
        .feature_draft_construction_graph_payloads
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_draft_construction_graph_payloads,
        )?;
    }
    Ok(())
}

fn emit_features_feature_draft_construction_fixed_lanes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_draft_construction_fixed_lanes.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_draft_construction_fixed_lanes,
        )?;
    }
    Ok(())
}

fn emit_features_feature_draft_construction_binary32_lanes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .features
        .feature_draft_construction_binary32_lanes
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_draft_construction_binary32_lanes,
        )?;
    }
    Ok(())
}

fn emit_features_feature_draft_construction_graph_strings(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .features
        .feature_draft_construction_graph_strings
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_draft_construction_graph_strings,
        )?;
    }
    Ok(())
}

fn emit_features_feature_draft_construction_identity_frames(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .features
        .feature_draft_construction_identity_frames
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_draft_construction_identity_frames,
        )?;
    }
    Ok(())
}

fn emit_features_feature_draft_construction_terminal_lanes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .features
        .feature_draft_construction_terminal_lanes
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_draft_construction_terminal_lanes,
        )?;
    }
    Ok(())
}

fn emit_features_feature_surface_construction_references(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .features
        .feature_surface_construction_references
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_surface_construction_references,
        )?;
    }
    Ok(())
}

fn emit_features_feature_surface_construction_payloads(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_surface_construction_payloads.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_surface_construction_payloads,
        )?;
    }
    Ok(())
}

fn emit_features_feature_surface_construction_scalar_pairs(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .features
        .feature_surface_construction_scalar_pairs
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_surface_construction_scalar_pairs,
        )?;
    }
    Ok(())
}

fn emit_features_feature_surface_construction_strings(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_surface_construction_strings.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_surface_construction_strings,
        )?;
    }
    Ok(())
}

fn emit_features_feature_surface_construction_branches(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_surface_construction_branches.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_surface_construction_branches,
        )?;
    }
    Ok(())
}

fn emit_features_feature_extrude_profile_references(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_extrude_profile_references.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_extrude_profile_references,
        )?;
    }
    Ok(())
}

fn emit_features_feature_extrude_payload_headers(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_extrude_payload_headers.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_extrude_payload_headers,
        )?;
    }
    Ok(())
}

fn emit_features_feature_extrude_payload_footers(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_extrude_payload_footers.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_extrude_payload_footers,
        )?;
    }
    Ok(())
}

fn emit_features_feature_operation_body_scalar_triples(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_operation_body_scalar_triples.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_operation_body_scalar_triples,
        )?;
    }
    Ok(())
}

fn emit_features_feature_operation_body_members(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_operation_body_members.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_operation_body_members,
        )?;
    }
    Ok(())
}

fn emit_features_feature_operation_body_operands(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_operation_body_operands.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_operation_body_operands,
        )?;
    }
    Ok(())
}

fn emit_features_feature_operation_body_11_continuations(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .features
        .feature_operation_body_11_continuations
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_operation_body_11_continuations,
        )?;
    }
    Ok(())
}

fn emit_features_feature_operation_body_reference_lanes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_operation_body_reference_lanes.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_operation_body_reference_lanes,
        )?;
    }
    Ok(())
}

fn emit_features_feature_extrude_construction_profiles(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_extrude_construction_profiles.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_extrude_construction_profiles,
        )?;
    }
    Ok(())
}

fn emit_features_feature_extrude_payload_32_branches(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_extrude_payload_32_branches.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_extrude_payload_32_branches,
        )?;
    }
    Ok(())
}

fn emit_features_feature_extrude_32_constructions(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_extrude_32_constructions.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_extrude_32_constructions,
        )?;
    }
    Ok(())
}

fn emit_features_feature_block_construction_references(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_block_construction_references.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_block_construction_references,
        )?;
    }
    Ok(())
}

fn emit_features_feature_block_constructions(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_block_constructions.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.feature_block_constructions)?;
    }
    Ok(())
}

fn emit_features_feature_block_construction_payloads(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_block_construction_payloads.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_block_construction_payloads,
        )?;
    }
    Ok(())
}

fn emit_features_feature_block_payload_scalars(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_block_payload_scalars.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_block_payload_scalars,
        )?;
    }
    Ok(())
}

fn emit_features_feature_block_payload_names(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_block_payload_names.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.feature_block_payload_names)?;
    }
    Ok(())
}

fn emit_features_feature_block_payload_named_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_block_payload_named_records.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_block_payload_named_records,
        )?;
    }
    Ok(())
}

fn emit_features_feature_block_payload_points(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_block_payload_points.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_block_payload_points,
        )?;
    }
    Ok(())
}

fn emit_features_feature_block_payload_point_groups(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_block_payload_point_groups.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_block_payload_point_groups,
        )?;
    }
    Ok(())
}

fn emit_features_feature_block_dimensions(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_block_dimensions.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.feature_block_dimensions)?;
    }
    Ok(())
}

fn emit_features_feature_sketch_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_sketch_records.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.feature_sketch_records)?;
    }
    Ok(())
}

fn emit_features_feature_sketch_construction_inputs(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_sketch_construction_inputs.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_sketch_construction_inputs,
        )?;
    }
    Ok(())
}

fn emit_features_feature_sketch_construction_payloads(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_sketch_construction_payloads.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_sketch_construction_payloads,
        )?;
    }
    Ok(())
}

fn emit_features_feature_sketch_payload_coordinate_pairs(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .features
        .feature_sketch_payload_coordinate_pairs
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_sketch_payload_coordinate_pairs,
        )?;
    }
    Ok(())
}

fn emit_features_feature_sketch_payload_fixed_pairs(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_sketch_payload_fixed_pairs.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_sketch_payload_fixed_pairs,
        )?;
    }
    Ok(())
}

fn emit_features_feature_sketch_payload_scalars(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_sketch_payload_scalars.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_sketch_payload_scalars,
        )?;
    }
    Ok(())
}

fn emit_features_feature_sketch_payload_names(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_sketch_payload_names.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_sketch_payload_names,
        )?;
    }
    Ok(())
}

fn emit_features_feature_sketch_payload_named_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_sketch_payload_named_records.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_sketch_payload_named_records,
        )?;
    }
    Ok(())
}

fn emit_features_feature_sketch_points(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_sketch_points.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.feature_sketch_points)?;
    }
    Ok(())
}

fn emit_features_feature_sketch_fixed_points(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_sketch_fixed_points.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.feature_sketch_fixed_points)?;
    }
    Ok(())
}

fn emit_features_feature_sketch_point_groups(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_sketch_point_groups.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.feature_sketch_point_groups)?;
    }
    Ok(())
}

fn emit_features_offset_store_named_points(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.offset_store_named_points.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.offset_store_named_points)?;
    }
    Ok(())
}

fn emit_features_feature_sketch_named_point_block_uses(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_sketch_named_point_block_uses.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_sketch_named_point_block_uses,
        )?;
    }
    Ok(())
}

fn emit_features_feature_sketch_preceding_named_point_uses(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m
        .features
        .feature_sketch_preceding_named_point_uses
        .is_empty()
    {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_sketch_preceding_named_point_uses,
        )?;
    }
    Ok(())
}

fn emit_features_feature_sketch_point_uses(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_sketch_point_uses.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.feature_sketch_point_uses)?;
    }
    Ok(())
}

fn emit_features_feature_sketch_datum_csys_dependencies(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_sketch_datum_csys_dependencies.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_sketch_datum_csys_dependencies,
        )?;
    }
    Ok(())
}

fn emit_features_feature_boolean_operations(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_boolean_operations.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.feature_boolean_operations)?;
    }
    Ok(())
}

fn emit_om_expression_declarations(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.expression_declarations.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.expression_declarations)?;
    }
    Ok(())
}

fn emit_features_data_block_object_frames(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.data_block_object_frames.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.data_block_object_frames)?;
    }
    Ok(())
}

fn emit_om_expressions(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.expressions.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.expressions)?;
    }
    Ok(())
}

fn emit_om_classes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.classes.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.classes)?;
    }
    Ok(())
}

fn emit_om_fields(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.fields.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.fields)?;
    }
    Ok(())
}

fn emit_om_object_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.object_records.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.object_records)?;
    }
    Ok(())
}

fn emit_om_rmfastload_object_id_tables(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.rmfastload_object_id_tables.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.rmfastload_object_id_tables)?;
    }
    Ok(())
}

fn emit_om_rmfastload_object_ids(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.rmfastload_object_ids.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.rmfastload_object_ids)?;
    }
    Ok(())
}

fn emit_om_data_blocks(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.data_blocks.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.data_blocks)?;
    }
    Ok(())
}

fn emit_om_data_block_control_values(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.data_block_control_values.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.data_block_control_values)?;
    }
    Ok(())
}

fn emit_om_data_block_control_class_references(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.data_block_control_class_references.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.om.data_block_control_class_references,
        )?;
    }
    Ok(())
}

fn emit_om_data_block_control_index_values(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.data_block_control_index_values.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.data_block_control_index_values)?;
    }
    Ok(())
}

fn emit_om_data_block_control_references(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.data_block_control_references.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.data_block_control_references)?;
    }
    Ok(())
}

fn emit_om_data_block_control_handle_pairs(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.data_block_control_handle_pairs.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.data_block_control_handle_pairs)?;
    }
    Ok(())
}

fn emit_om_data_block_references(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.data_block_references.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.data_block_references)?;
    }
    Ok(())
}

fn emit_om_data_block_counted_index_lanes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.data_block_counted_index_lanes.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.data_block_counted_index_lanes)?;
    }
    Ok(())
}

fn emit_om_data_block_abr_reference_lanes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.data_block_abr_reference_lanes.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.data_block_abr_reference_lanes)?;
    }
    Ok(())
}

fn emit_om_data_block_index_rows(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.data_block_index_rows.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.data_block_index_rows)?;
    }
    Ok(())
}

fn emit_om_data_block_linked_index_rows(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.data_block_linked_index_rows.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.data_block_linked_index_rows)?;
    }
    Ok(())
}

fn emit_om_data_block_target_index_rows(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.data_block_target_index_rows.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.data_block_target_index_rows)?;
    }
    Ok(())
}

fn emit_om_data_block_column_index_tables(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.data_block_column_index_tables.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.data_block_column_index_tables)?;
    }
    Ok(())
}

fn emit_features_feature_input_column_row_uses(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_input_column_row_uses.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_input_column_row_uses,
        )?;
    }
    Ok(())
}

fn emit_features_feature_input_column_targets(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_input_column_targets.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.features.feature_input_column_targets,
        )?;
    }
    Ok(())
}

fn emit_features_feature_parameter_bindings(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_parameter_bindings.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.feature_parameter_bindings)?;
    }
    Ok(())
}

fn emit_features_feature_parameter_uses(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.features.feature_parameter_uses.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.features.feature_parameter_uses)?;
    }
    Ok(())
}

fn emit_om_store_headers(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.store_headers.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.store_headers)?;
    }
    Ok(())
}

fn emit_om_string_values(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.string_values.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.string_values)?;
    }
    Ok(())
}

fn emit_om_object_references(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.object_references.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.object_references)?;
    }
    Ok(())
}

fn emit_om_persistent_handles(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.persistent_handles.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.persistent_handles)?;
    }
    Ok(())
}

fn emit_om_configurations(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.configurations.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.configurations)?;
    }
    Ok(())
}

fn emit_om_configuration_attribute_uses(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.configuration_attribute_uses.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.configuration_attribute_uses)?;
    }
    Ok(())
}

fn emit_om_part_attributes(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.part_attributes.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.part_attributes)?;
    }
    Ok(())
}

fn emit_om_external_references(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.external_references.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.external_references)?;
    }
    Ok(())
}

fn emit_om_external_reference_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.external_reference_records.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.external_reference_records)?;
    }
    Ok(())
}

fn emit_om_external_reference_indexed_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.external_reference_indexed_records.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.om.external_reference_indexed_records,
        )?;
    }
    Ok(())
}

fn emit_om_external_reference_empty_records(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.external_reference_empty_records.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.external_reference_empty_records)?;
    }
    Ok(())
}

fn emit_om_external_reference_tail_reference_pairs(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.external_reference_tail_reference_pairs.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.om.external_reference_tail_reference_pairs,
        )?;
    }
    Ok(())
}

fn emit_om_external_reference_record_string_uses(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.external_reference_record_string_uses.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.om.external_reference_record_string_uses,
        )?;
    }
    Ok(())
}

fn emit_om_external_reference_record_children(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.external_reference_record_children.is_empty() {
        ns.set_arena(
            catalogue_row.arena,
            &m.om.external_reference_record_children,
        )?;
    }
    Ok(())
}

fn emit_om_material_texture_assets(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.material_texture_assets.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.material_texture_assets)?;
    }
    Ok(())
}

fn emit_om_material_texture_catalog_entries(
    m: &NativeModel,
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !m.om.material_texture_catalog_entries.is_empty() {
        ns.set_arena(catalogue_row.arena, &m.om.material_texture_catalog_entries)?;
    }
    Ok(())
}

/// One row per native record family, note-bearing rows in emission order.
pub(crate) const CATALOGUE: &[CatalogueRow] = &[
    CatalogueRow {
        arena: "display_jt_indices",
        tag: None,
        exactness: Exactness::ByteExact,
        note: Some(note_display_jt_display_jt_indices),
        emit: emit_display_jt_display_jt_indices,
        len: |m| m.display_jt.display_jt_indices.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "display_jt_documents",
        tag: None,
        exactness: Exactness::ByteExact,
        note: Some(note_display_jt_display_jt_documents),
        emit: emit_display_jt_display_jt_documents,
        len: |m| m.display_jt.display_jt_documents.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_segments",
        tag: Some("DISPLAY_JT_SEGMENT"),
        exactness: Exactness::ByteExact,
        note: Some(note_display_jt_display_jt_segments),
        emit: emit_display_jt_display_jt_segments,
        len: |m| m.display_jt.display_jt_segments.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_shape_lod_elements",
        tag: Some("DISPLAY_JT_SHAPE_LOD_ELEMENT"),
        exactness: Exactness::ByteExact,
        note: Some(note_display_jt_display_jt_shape_lod_elements),
        emit: emit_display_jt_display_jt_shape_lod_elements,
        len: |m| m.display_jt.display_jt_shape_lod_elements.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_tri_strip_lod_headers",
        tag: Some("DISPLAY_JT_TRI_STRIP_LOD_HEADER"),
        exactness: Exactness::ByteExact,
        note: Some(note_display_jt_display_jt_tri_strip_lod_headers),
        emit: emit_display_jt_display_jt_tri_strip_lod_headers,
        len: |m| m.display_jt.display_jt_tri_strip_lod_headers.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_initial_face_degree_symbols",
        tag: Some("DISPLAY_JT_INITIAL_FACE_DEGREE_SYMBOLS"),
        exactness: Exactness::ByteExact,
        note: Some(note_display_jt_display_jt_initial_face_degree_symbols),
        emit: emit_display_jt_display_jt_initial_face_degree_symbols,
        len: |m| m.display_jt.display_jt_initial_face_degree_symbols.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_topology_packet_sequences",
        tag: Some("DISPLAY_JT_TOPOLOGY_PACKET_SEQUENCE"),
        exactness: Exactness::ByteExact,
        note: Some(note_display_jt_display_jt_topology_packet_sequences),
        emit: emit_display_jt_display_jt_topology_packet_sequences,
        len: |m| m.display_jt.display_jt_topology_packet_sequences.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_vertex_records_headers",
        tag: Some("DISPLAY_JT_VERTEX_RECORDS_HEADER"),
        exactness: Exactness::ByteExact,
        note: Some(note_display_jt_display_jt_vertex_records_headers),
        emit: emit_display_jt_display_jt_vertex_records_headers,
        len: |m| m.display_jt.display_jt_vertex_records_headers.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_coordinate_array_headers",
        tag: Some("DISPLAY_JT_COORDINATE_ARRAY_HEADER"),
        exactness: Exactness::ByteExact,
        note: Some(note_display_jt_display_jt_coordinate_array_headers),
        emit: emit_display_jt_display_jt_coordinate_array_headers,
        len: |m| m.display_jt.display_jt_coordinate_array_headers.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_vertex_coordinates",
        tag: Some("DISPLAY_JT_VERTEX_COORDINATES"),
        exactness: Exactness::Derived,
        note: Some(note_display_jt_display_jt_vertex_coordinates),
        emit: emit_display_jt_display_jt_vertex_coordinates,
        len: |m| m.display_jt.display_jt_vertex_coordinates.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_vertex_normals",
        tag: Some("DISPLAY_JT_VERTEX_NORMALS"),
        exactness: Exactness::Derived,
        note: Some(note_display_jt_display_jt_vertex_normals),
        emit: emit_display_jt_display_jt_vertex_normals,
        len: |m| m.display_jt.display_jt_vertex_normals.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_vertex_colors",
        tag: Some("DISPLAY_JT_VERTEX_COLORS"),
        exactness: Exactness::Derived,
        note: Some(note_display_jt_display_jt_vertex_colors),
        emit: emit_display_jt_display_jt_vertex_colors,
        len: |m| m.display_jt.display_jt_vertex_colors.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_vertex_texture_coordinates",
        tag: Some("DISPLAY_JT_VERTEX_TEXTURE_COORDINATES"),
        exactness: Exactness::Derived,
        note: Some(note_display_jt_display_jt_vertex_texture_coordinates),
        emit: emit_display_jt_display_jt_vertex_texture_coordinates,
        len: |m| m.display_jt.display_jt_vertex_texture_coordinates.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_vertex_flags",
        tag: Some("DISPLAY_JT_VERTEX_FLAGS"),
        exactness: Exactness::Derived,
        note: Some(note_display_jt_display_jt_vertex_flags),
        emit: emit_display_jt_display_jt_vertex_flags,
        len: |m| m.display_jt.display_jt_vertex_flags.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_geometric_transform_attributes",
        tag: Some("DISPLAY_JT_GEOMETRIC_TRANSFORM"),
        exactness: Exactness::Derived,
        note: Some(note_display_jt_display_jt_geometric_transform_attributes),
        emit: emit_display_jt_display_jt_geometric_transform_attributes,
        len: |m| m.display_jt.display_jt_geometric_transform_attributes.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_polygon_meshes",
        tag: Some("DISPLAY_JT_POLYGON_MESH"),
        exactness: Exactness::Derived,
        note: Some(note_display_jt_display_jt_polygon_meshes),
        emit: emit_display_jt_display_jt_polygon_meshes,
        len: |m| m.display_jt.display_jt_polygon_meshes.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_compressed_element_sequences",
        tag: Some("DISPLAY_JT_COMPRESSED_ELEMENT_SEQUENCE"),
        exactness: Exactness::ByteExact,
        note: Some(note_display_jt_display_jt_compressed_element_sequences),
        emit: emit_display_jt_display_jt_compressed_element_sequences,
        len: |m| m.display_jt.display_jt_compressed_element_sequences.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_compressed_elements",
        tag: Some("DISPLAY_JT_COMPRESSED_ELEMENT"),
        exactness: Exactness::ByteExact,
        note: Some(note_display_jt_display_jt_compressed_elements),
        emit: emit_display_jt_display_jt_compressed_elements,
        len: |m| m.display_jt.display_jt_compressed_elements.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_string_property_atoms",
        tag: Some("DISPLAY_JT_STRING_PROPERTY_ATOM"),
        exactness: Exactness::ByteExact,
        note: Some(note_display_jt_display_jt_string_property_atoms),
        emit: emit_display_jt_display_jt_string_property_atoms,
        len: |m| m.display_jt.display_jt_string_property_atoms.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_shape_lod_bindings",
        tag: Some("DISPLAY_JT_SHAPE_LOD_BINDING"),
        exactness: Exactness::ByteExact,
        note: Some(note_display_jt_display_jt_shape_lod_bindings),
        emit: emit_display_jt_display_jt_shape_lod_bindings,
        len: |m| m.display_jt.display_jt_shape_lod_bindings.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_base_node_data",
        tag: Some("DISPLAY_JT_BASE_NODE_DATA"),
        exactness: Exactness::ByteExact,
        note: Some(note_display_jt_display_jt_base_node_data),
        emit: emit_display_jt_display_jt_base_node_data,
        len: |m| m.display_jt.display_jt_base_node_data.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_group_node_data",
        tag: Some("DISPLAY_JT_GROUP_NODE_DATA"),
        exactness: Exactness::ByteExact,
        note: Some(note_display_jt_display_jt_group_node_data),
        emit: emit_display_jt_display_jt_group_node_data,
        len: |m| m.display_jt.display_jt_group_node_data.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_instance_nodes",
        tag: Some("DISPLAY_JT_INSTANCE_NODE"),
        exactness: Exactness::ByteExact,
        note: Some(note_display_jt_display_jt_instance_nodes),
        emit: emit_display_jt_display_jt_instance_nodes,
        len: |m| m.display_jt.display_jt_instance_nodes.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_partition_nodes",
        tag: Some("DISPLAY_JT_PARTITION_NODE"),
        exactness: Exactness::ByteExact,
        note: Some(note_display_jt_display_jt_partition_nodes),
        emit: emit_display_jt_display_jt_partition_nodes,
        len: |m| m.display_jt.display_jt_partition_nodes.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_range_lod_nodes",
        tag: Some("DISPLAY_JT_RANGE_LOD_NODE"),
        exactness: Exactness::ByteExact,
        note: Some(note_display_jt_display_jt_range_lod_nodes),
        emit: emit_display_jt_display_jt_range_lod_nodes,
        len: |m| m.display_jt.display_jt_range_lod_nodes.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_tri_strip_shape_nodes",
        tag: Some("DISPLAY_JT_TRI_STRIP_SHAPE_NODE"),
        exactness: Exactness::ByteExact,
        note: Some(note_display_jt_display_jt_tri_strip_shape_nodes),
        emit: emit_display_jt_display_jt_tri_strip_shape_nodes,
        len: |m| m.display_jt.display_jt_tri_strip_shape_nodes.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "segment_index_rows",
        tag: Some("UG_PART_SEGMENT_INDEX_ROW"),
        exactness: Exactness::ByteExact,
        note: Some(note_segments_segment_index_rows),
        emit: emit_segments_segment_index_rows,
        len: |m| m.segments.segment_index_rows.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "segment_stream_links",
        tag: Some("UG_PART_SEGMENT_STREAM_LINK"),
        exactness: Exactness::ByteExact,
        note: Some(note_segments_segment_stream_links),
        emit: emit_segments_segment_stream_links,
        len: |m| m.segments.segment_stream_links.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "segment_body_bindings",
        tag: Some("UG_PART_SEGMENT_BODY_BINDING"),
        exactness: Exactness::ByteExact,
        note: Some(note_segments_segment_body_bindings),
        emit: emit_segments_segment_body_bindings,
        len: |m| m.segments.segment_body_bindings.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "segment_body_lineage_statuses",
        tag: Some("SEGMENT_BODY_LINEAGE_STATUS"),
        exactness: Exactness::Derived,
        note: Some(note_segments_segment_body_lineage_statuses),
        emit: emit_segments_segment_body_lineage_statuses,
        len: |m| m.segments.segment_body_lineage_statuses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_blend_surface_records",
        tag: Some("BLEND_SURF"),
        exactness: Exactness::ByteExact,
        note: Some(note_parasolid_parasolid_blend_surface_records),
        emit: emit_parasolid_parasolid_blend_surface_records,
        len: |m| m.parasolid.parasolid_blend_surface_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_blend_bound_records",
        tag: Some("BLEND_BOUND"),
        exactness: Exactness::ByteExact,
        note: Some(note_parasolid_parasolid_blend_bound_records),
        emit: emit_parasolid_parasolid_blend_bound_records,
        len: |m| m.parasolid.parasolid_blend_bound_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_offset_surface_records",
        tag: Some("OFFSET_SURF"),
        exactness: Exactness::ByteExact,
        note: Some(note_parasolid_parasolid_offset_surface_records),
        emit: emit_parasolid_parasolid_offset_surface_records,
        len: |m| m.parasolid.parasolid_offset_surface_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_trimmed_curve_records",
        tag: Some("TRIMMED_CURVE"),
        exactness: Exactness::ByteExact,
        note: Some(note_parasolid_parasolid_trimmed_curve_records),
        emit: emit_parasolid_parasolid_trimmed_curve_records,
        len: |m| m.parasolid.parasolid_trimmed_curve_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_surface_curve_records",
        tag: Some("SP_CURVE"),
        exactness: Exactness::ByteExact,
        note: Some(note_parasolid_parasolid_surface_curve_records),
        emit: emit_parasolid_parasolid_surface_curve_records,
        len: |m| m.parasolid.parasolid_surface_curve_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_intersection_records",
        tag: None,
        exactness: Exactness::ByteExact,
        note: Some(note_parasolid_parasolid_intersection_records),
        emit: emit_parasolid_parasolid_intersection_records,
        len: |m| m.parasolid.parasolid_intersection_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_term_use_records",
        tag: Some("term_use"),
        exactness: Exactness::ByteExact,
        note: Some(note_parasolid_parasolid_term_use_records),
        emit: emit_parasolid_parasolid_term_use_records,
        len: |m| m.parasolid.parasolid_term_use_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_support_uv_records",
        tag: Some("values"),
        exactness: Exactness::ByteExact,
        note: Some(note_parasolid_parasolid_support_uv_records),
        emit: emit_parasolid_parasolid_support_uv_records,
        len: |m| m.parasolid.parasolid_support_uv_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_chart_records",
        tag: Some("CHART_s"),
        exactness: Exactness::ByteExact,
        note: Some(note_parasolid_parasolid_chart_records),
        emit: emit_parasolid_parasolid_chart_records,
        len: |m| m.parasolid.parasolid_chart_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_attribute_definitions",
        tag: Some("ATTRIBUTE_DEFINITION"),
        exactness: Exactness::ByteExact,
        note: Some(note_parasolid_parasolid_attribute_definitions),
        emit: emit_parasolid_parasolid_attribute_definitions,
        len: |m| m.parasolid.parasolid_attribute_definitions.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_51_records",
        tag: Some("ENTITY_51"),
        exactness: Exactness::ByteExact,
        note: Some(note_parasolid_parasolid_entity_51_records),
        emit: emit_parasolid_parasolid_entity_51_records,
        len: |m| m.parasolid.parasolid_entity_51_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_52_integer_records",
        tag: Some("ENTITY_52_INTEGERS"),
        exactness: Exactness::ByteExact,
        note: Some(note_parasolid_parasolid_entity_52_integer_records),
        emit: emit_parasolid_parasolid_entity_52_integer_records,
        len: |m| m.parasolid.parasolid_entity_52_integer_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_53_double_records",
        tag: Some("ENTITY_53_DOUBLES"),
        exactness: Exactness::ByteExact,
        note: Some(note_parasolid_parasolid_entity_53_double_records),
        emit: emit_parasolid_parasolid_entity_53_double_records,
        len: |m| m.parasolid.parasolid_entity_53_double_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_54_string_records",
        tag: Some("ENTITY_54_STRING"),
        exactness: Exactness::ByteExact,
        note: Some(note_parasolid_parasolid_entity_54_string_records),
        emit: emit_parasolid_parasolid_entity_54_string_records,
        len: |m| m.parasolid.parasolid_entity_54_string_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_51_string_uses",
        tag: Some("ENTITY_51_STRING_USE"),
        exactness: Exactness::ByteExact,
        note: Some(note_parasolid_parasolid_entity_51_string_uses),
        emit: emit_parasolid_parasolid_entity_51_string_uses,
        len: |m| m.parasolid.parasolid_entity_51_string_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_51_numeric_uses",
        tag: Some("ENTITY_51_NUMERIC_USE"),
        exactness: Exactness::ByteExact,
        note: Some(note_parasolid_parasolid_entity_51_numeric_uses),
        emit: emit_parasolid_parasolid_entity_51_numeric_uses,
        len: |m| m.parasolid.parasolid_entity_51_numeric_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_attribute_class_uses",
        tag: None,
        exactness: Exactness::Derived,
        note: Some(note_parasolid_parasolid_attribute_class_uses),
        emit: emit_parasolid_parasolid_attribute_class_uses,
        len: |m| m.parasolid.parasolid_attribute_class_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_topology_attribute_list_references",
        tag: Some("TOPOLOGY_ATTRIBUTE_LIST_REFERENCE"),
        exactness: Exactness::ByteExact,
        note: Some(note_parasolid_parasolid_topology_attribute_list_references),
        emit: emit_parasolid_parasolid_topology_attribute_list_references,
        len: |m| {
            m.parasolid
                .parasolid_topology_attribute_list_references
                .len()
        },
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_topology_attribute_class_uses",
        tag: None,
        exactness: Exactness::Derived,
        note: Some(note_parasolid_parasolid_topology_attribute_class_uses),
        emit: emit_parasolid_parasolid_topology_attribute_class_uses,
        len: |m| m.parasolid.parasolid_topology_attribute_class_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "data_block_object_frames",
        tag: Some("OFFSET_STORE_OBJECT_FRAME"),
        exactness: Exactness::ByteExact,
        note: Some(note_features_data_block_object_frames),
        emit: emit_features_data_block_object_frames,
        len: |m| m.features.data_block_object_frames.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "offset_store_named_points",
        tag: Some("OFFSET_STORE_NAMED_POINT"),
        exactness: Exactness::ByteExact,
        note: Some(note_features_offset_store_named_points),
        emit: emit_features_offset_store_named_points,
        len: |m| m.features.offset_store_named_points.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_named_point_block_uses",
        tag: Some("SKETCH_NAMED_POINT_BLOCK_USE"),
        exactness: Exactness::ByteExact,
        note: Some(note_features_feature_sketch_named_point_block_uses),
        emit: emit_features_feature_sketch_named_point_block_uses,
        len: |m| m.features.feature_sketch_named_point_block_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_preceding_named_point_uses",
        tag: Some("SKETCH_PRECEDING_NAMED_POINT_USE"),
        exactness: Exactness::ByteExact,
        note: Some(note_features_feature_sketch_preceding_named_point_uses),
        emit: emit_features_feature_sketch_preceding_named_point_uses,
        len: |m| m.features.feature_sketch_preceding_named_point_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_sketch_point_uses",
        tag: None,
        exactness: Exactness::Derived,
        note: Some(note_features_feature_sketch_point_uses),
        emit: emit_features_feature_sketch_point_uses,
        len: |m| m.features.feature_sketch_point_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_datum_csys_dependencies",
        tag: Some("SKETCH_DATUM_CSYS_DEPENDENCY"),
        exactness: Exactness::Derived,
        note: Some(note_features_feature_sketch_datum_csys_dependencies),
        emit: emit_features_feature_sketch_datum_csys_dependencies,
        len: |m| m.features.feature_sketch_datum_csys_dependencies.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_input_block_identity_groups",
        tag: None,
        exactness: Exactness::ByteExact,
        note: Some(note_features_feature_input_block_identity_groups),
        emit: emit_features_feature_input_block_identity_groups,
        len: |m| m.features.feature_input_block_identity_groups.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_abr_reference_lanes",
        tag: Some("OFFSET_STORE_ABR_REFERENCE_LANE"),
        exactness: Exactness::ByteExact,
        note: Some(note_om_data_block_abr_reference_lanes),
        emit: emit_om_data_block_abr_reference_lanes,
        len: |m| m.om.data_block_abr_reference_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "segment_om_links",
        tag: Some("UG_PART_SEGMENT_OM_LINK"),
        exactness: Exactness::ByteExact,
        note: Some(note_segments_segment_om_links),
        emit: emit_segments_segment_om_links,
        len: |m| m.segments.segment_om_links.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "om_record_areas",
        tag: Some("OM_RECORD_AREA"),
        exactness: Exactness::ByteExact,
        note: Some(note_om_om_record_areas),
        emit: emit_om_om_record_areas,
        len: |m| m.om.om_record_areas.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_labels",
        tag: Some("FEATURE_OPERATION_LABEL"),
        exactness: Exactness::ByteExact,
        note: Some(note_features_feature_operation_labels),
        emit: emit_features_feature_operation_labels,
        len: |m| m.features.feature_operation_labels.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_records",
        tag: Some("FEATURE_SKETCH_RECORD"),
        exactness: Exactness::Derived,
        note: Some(note_features_feature_sketch_records),
        emit: emit_features_feature_sketch_records,
        len: |m| m.features.feature_sketch_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_payload_fixed_pairs",
        tag: Some("FEATURE_SKETCH_FIXED_PAIR"),
        exactness: Exactness::ByteExact,
        note: Some(note_features_feature_sketch_payload_fixed_pairs),
        emit: emit_features_feature_sketch_payload_fixed_pairs,
        len: |m| m.features.feature_sketch_payload_fixed_pairs.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_fixed_points",
        tag: Some("FEATURE_SKETCH_FIXED_POINT"),
        exactness: Exactness::Derived,
        note: Some(note_features_feature_sketch_fixed_points),
        emit: emit_features_feature_sketch_fixed_points,
        len: |m| m.features.feature_sketch_fixed_points.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_records",
        tag: Some("FEATURE_OPERATION_RECORD"),
        exactness: Exactness::ByteExact,
        note: Some(note_features_feature_operation_records),
        emit: emit_features_feature_operation_records,
        len: |m| m.features.feature_operation_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_payload_strings",
        tag: Some("FEATURE_PAYLOAD_STRING"),
        exactness: Exactness::ByteExact,
        note: Some(note_features_feature_payload_strings),
        emit: emit_features_feature_payload_strings,
        len: |m| m.features.feature_payload_strings.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_body_references",
        tag: Some("FEATURE_BODY_REFERENCE"),
        exactness: Exactness::ByteExact,
        note: Some(note_features_feature_body_references),
        emit: emit_features_feature_body_references,
        len: |m| m.features.feature_body_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_body_reference_occurrences",
        tag: Some("FEATURE_BODY_REFERENCE_OCCURRENCE"),
        exactness: Exactness::ByteExact,
        note: Some(note_features_feature_body_reference_occurrences),
        emit: emit_features_feature_body_reference_occurrences,
        len: |m| m.features.feature_body_reference_occurrences.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_input_blocks",
        tag: Some("FEATURE_INPUT_BLOCK"),
        exactness: Exactness::ByteExact,
        note: Some(note_features_feature_input_blocks),
        emit: emit_features_feature_input_blocks,
        len: |m| m.features.feature_input_blocks.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_boolean_operations",
        tag: Some("FEATURE_BOOLEAN_OPERATION"),
        exactness: Exactness::ByteExact,
        note: Some(note_features_feature_boolean_operations),
        emit: emit_features_feature_boolean_operations,
        len: |m| m.features.feature_boolean_operations.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "expression_declarations",
        tag: Some("EXPRESSION_DECLARATION"),
        exactness: Exactness::ByteExact,
        note: Some(note_om_expression_declarations),
        emit: emit_om_expression_declarations,
        len: |m| m.om.expression_declarations.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_control_values",
        tag: Some("OM_DATA_BLOCK_CONTROL_VALUE"),
        exactness: Exactness::ByteExact,
        note: Some(note_om_data_block_control_values),
        emit: emit_om_data_block_control_values,
        len: |m| m.om.data_block_control_values.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_control_class_references",
        tag: Some("OM_DATA_BLOCK_CONTROL_CLASS_REFERENCE"),
        exactness: Exactness::ByteExact,
        note: Some(note_om_data_block_control_class_references),
        emit: emit_om_data_block_control_class_references,
        len: |m| m.om.data_block_control_class_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_control_index_values",
        tag: Some("OM_DATA_BLOCK_CONTROL_INDEX_VALUE"),
        exactness: Exactness::ByteExact,
        note: Some(note_om_data_block_control_index_values),
        emit: emit_om_data_block_control_index_values,
        len: |m| m.om.data_block_control_index_values.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_control_references",
        tag: Some("OM_DATA_BLOCK_CONTROL_REFERENCE"),
        exactness: Exactness::ByteExact,
        note: Some(note_om_data_block_control_references),
        emit: emit_om_data_block_control_references,
        len: |m| m.om.data_block_control_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_control_handle_pairs",
        tag: Some("OM_DATA_BLOCK_CONTROL_HANDLE_PAIR"),
        exactness: Exactness::ByteExact,
        note: Some(note_om_data_block_control_handle_pairs),
        emit: emit_om_data_block_control_handle_pairs,
        len: |m| m.om.data_block_control_handle_pairs.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_references",
        tag: Some("OM_DATA_BLOCK_REFERENCE"),
        exactness: Exactness::ByteExact,
        note: Some(note_om_data_block_references),
        emit: emit_om_data_block_references,
        len: |m| m.om.data_block_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_parameter_bindings",
        tag: Some("FEATURE_PARAMETER_BINDING"),
        exactness: Exactness::Derived,
        note: Some(note_features_feature_parameter_bindings),
        emit: emit_features_feature_parameter_bindings,
        len: |m| m.features.feature_parameter_bindings.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_parameter_uses",
        tag: None,
        exactness: Exactness::Derived,
        note: Some(note_features_feature_parameter_uses),
        emit: emit_features_feature_parameter_uses,
        len: |m| m.features.feature_parameter_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "store_headers",
        tag: Some("OM_STORE_VERSION"),
        exactness: Exactness::ByteExact,
        note: Some(note_om_store_headers),
        emit: emit_om_store_headers,
        len: |m| m.om.store_headers.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "external_references",
        tag: Some("EXTREFSTREAM_STRING"),
        exactness: Exactness::ByteExact,
        note: Some(note_om_external_references),
        emit: emit_om_external_references,
        len: |m| m.om.external_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "external_reference_records",
        tag: Some("EXTREFSTREAM_RECORD"),
        exactness: Exactness::ByteExact,
        note: Some(note_om_external_reference_records),
        emit: emit_om_external_reference_records,
        len: |m| m.om.external_reference_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "material_texture_assets",
        tag: Some("TIFF_MATERIAL_TEXTURE"),
        exactness: Exactness::ByteExact,
        note: Some(note_om_material_texture_assets),
        emit: emit_om_material_texture_assets,
        len: |m| m.om.material_texture_assets.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "material_texture_catalog_entries",
        tag: Some("QAF_MATERIAL_TEXTURE_CATALOG_ENTRY"),
        exactness: Exactness::Derived,
        note: Some(note_om_material_texture_catalog_entries),
        emit: emit_om_material_texture_catalog_entries,
        len: |m| m.om.material_texture_catalog_entries.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_body_segment_uses",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_body_segment_uses,
        len: |m| m.features.feature_body_segment_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_simple_hole_templates",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_simple_hole_templates,
        len: |m| m.features.feature_simple_hole_templates.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_simple_hole_repeated_scalar_lanes",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_simple_hole_repeated_scalar_lanes,
        len: |m| m.features.feature_simple_hole_repeated_scalar_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_simple_hole_repeated_scalar_lane_block_references",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_simple_hole_repeated_scalar_lane_block_references,
        len: |m| {
            m.features
                .feature_simple_hole_repeated_scalar_lane_block_references
                .len()
        },
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_simple_hole_construction_groups",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_simple_hole_construction_groups,
        len: |m| m.features.feature_simple_hole_construction_groups.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_datum_csys_constructions",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_datum_csys_constructions,
        len: |m| m.features.feature_datum_csys_constructions.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_datum_csys_payloads",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_datum_csys_payloads,
        len: |m| m.features.feature_datum_csys_payloads.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_datum_csys_payload_scalar_pairs",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_datum_csys_payload_scalar_pairs,
        len: |m| m.features.feature_datum_csys_payload_scalar_pairs.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_datum_csys_payload_fixed_pairs",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_datum_csys_payload_fixed_pairs,
        len: |m| m.features.feature_datum_csys_payload_fixed_pairs.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_datum_csys_payload_scalars",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_datum_csys_payload_scalars,
        len: |m| m.features.feature_datum_csys_payload_scalars.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_datum_csys_descriptors",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_datum_csys_descriptors,
        len: |m| m.features.feature_datum_csys_descriptors.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_datum_csys_block_uses",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_datum_csys_block_uses,
        len: |m| m.features.feature_datum_csys_block_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_datum_plane_headers",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_datum_plane_headers,
        len: |m| m.features.feature_datum_plane_headers.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_datum_plane_block_uses",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_datum_plane_block_uses,
        len: |m| m.features.feature_datum_plane_block_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_datum_plane_payloads",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_datum_plane_payloads,
        len: |m| m.features.feature_datum_plane_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_datum_plane_payload_scalar_pairs",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_datum_plane_payload_scalar_pairs,
        len: |m| m.features.feature_datum_plane_payload_scalar_pairs.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_datum_plane_descriptors",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_datum_plane_descriptors,
        len: |m| m.features.feature_datum_plane_descriptors.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_datum_plane_csys_identity_uses",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_datum_plane_csys_identity_uses,
        len: |m| m.features.feature_datum_plane_csys_identity_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_sketch_references",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_sketch_references,
        len: |m| m.features.feature_sketch_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_projected_curve_references",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_projected_curve_references,
        len: |m| m.features.feature_projected_curve_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_projected_curve_construction_payloads",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_projected_curve_construction_payloads,
        len: |m| {
            m.features
                .feature_projected_curve_construction_payloads
                .len()
        },
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_projected_curve_construction_strings",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_projected_curve_construction_strings,
        len: |m| {
            m.features
                .feature_projected_curve_construction_strings
                .len()
        },
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_pattern_references",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_pattern_references,
        len: |m| m.features.feature_pattern_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_pattern_construction_payloads",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_pattern_construction_payloads,
        len: |m| m.features.feature_pattern_construction_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_pattern_construction_strings",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_pattern_construction_strings,
        len: |m| m.features.feature_pattern_construction_strings.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_pattern_construction_fixed_lanes",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_pattern_construction_fixed_lanes,
        len: |m| m.features.feature_pattern_construction_fixed_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_pattern_transform_lanes",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_pattern_transform_lanes,
        len: |m| m.features.feature_pattern_transform_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_point_construction_headers",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_point_construction_headers,
        len: |m| m.features.feature_point_construction_headers.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_point_construction_scalar_lanes",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_point_construction_scalar_lanes,
        len: |m| m.features.feature_point_construction_scalar_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_references",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_draft_construction_references,
        len: |m| m.features.feature_draft_construction_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_index_lanes",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_draft_construction_index_lanes,
        len: |m| m.features.feature_draft_construction_index_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_payloads",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_draft_construction_payloads,
        len: |m| m.features.feature_draft_construction_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_graph_payloads",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_draft_construction_graph_payloads,
        len: |m| m.features.feature_draft_construction_graph_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_fixed_lanes",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_draft_construction_fixed_lanes,
        len: |m| m.features.feature_draft_construction_fixed_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_binary32_lanes",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_draft_construction_binary32_lanes,
        len: |m| m.features.feature_draft_construction_binary32_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_graph_strings",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_draft_construction_graph_strings,
        len: |m| m.features.feature_draft_construction_graph_strings.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_identity_frames",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_draft_construction_identity_frames,
        len: |m| m.features.feature_draft_construction_identity_frames.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_terminal_lanes",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_draft_construction_terminal_lanes,
        len: |m| m.features.feature_draft_construction_terminal_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_surface_construction_references",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_surface_construction_references,
        len: |m| m.features.feature_surface_construction_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_surface_construction_payloads",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_surface_construction_payloads,
        len: |m| m.features.feature_surface_construction_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_surface_construction_scalar_pairs",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_surface_construction_scalar_pairs,
        len: |m| m.features.feature_surface_construction_scalar_pairs.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_surface_construction_strings",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_surface_construction_strings,
        len: |m| m.features.feature_surface_construction_strings.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_surface_construction_branches",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_surface_construction_branches,
        len: |m| m.features.feature_surface_construction_branches.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_extrude_profile_references",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_extrude_profile_references,
        len: |m| m.features.feature_extrude_profile_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_extrude_payload_headers",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_extrude_payload_headers,
        len: |m| m.features.feature_extrude_payload_headers.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_extrude_payload_footers",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_extrude_payload_footers,
        len: |m| m.features.feature_extrude_payload_footers.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_body_scalar_triples",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_operation_body_scalar_triples,
        len: |m| m.features.feature_operation_body_scalar_triples.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_body_members",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_operation_body_members,
        len: |m| m.features.feature_operation_body_members.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_body_operands",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_operation_body_operands,
        len: |m| m.features.feature_operation_body_operands.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_body_11_continuations",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_operation_body_11_continuations,
        len: |m| m.features.feature_operation_body_11_continuations.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_body_reference_lanes",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_operation_body_reference_lanes,
        len: |m| m.features.feature_operation_body_reference_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_extrude_construction_profiles",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_extrude_construction_profiles,
        len: |m| m.features.feature_extrude_construction_profiles.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_extrude_payload_32_branches",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_extrude_payload_32_branches,
        len: |m| m.features.feature_extrude_payload_32_branches.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_extrude_32_constructions",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_extrude_32_constructions,
        len: |m| m.features.feature_extrude_32_constructions.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_construction_references",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_block_construction_references,
        len: |m| m.features.feature_block_construction_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_constructions",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_block_constructions,
        len: |m| m.features.feature_block_constructions.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_construction_payloads",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_block_construction_payloads,
        len: |m| m.features.feature_block_construction_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_payload_scalars",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_block_payload_scalars,
        len: |m| m.features.feature_block_payload_scalars.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_payload_names",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_block_payload_names,
        len: |m| m.features.feature_block_payload_names.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_payload_named_records",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_block_payload_named_records,
        len: |m| m.features.feature_block_payload_named_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_payload_points",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_block_payload_points,
        len: |m| m.features.feature_block_payload_points.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_payload_point_groups",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_block_payload_point_groups,
        len: |m| m.features.feature_block_payload_point_groups.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_dimensions",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_block_dimensions,
        len: |m| m.features.feature_block_dimensions.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_sketch_construction_inputs",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_sketch_construction_inputs,
        len: |m| m.features.feature_sketch_construction_inputs.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_construction_payloads",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_sketch_construction_payloads,
        len: |m| m.features.feature_sketch_construction_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_payload_coordinate_pairs",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_sketch_payload_coordinate_pairs,
        len: |m| m.features.feature_sketch_payload_coordinate_pairs.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_sketch_payload_scalars",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_sketch_payload_scalars,
        len: |m| m.features.feature_sketch_payload_scalars.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_payload_names",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_sketch_payload_names,
        len: |m| m.features.feature_sketch_payload_names.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_payload_named_records",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_sketch_payload_named_records,
        len: |m| m.features.feature_sketch_payload_named_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_points",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_sketch_points,
        len: |m| m.features.feature_sketch_points.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_point_groups",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_sketch_point_groups,
        len: |m| m.features.feature_sketch_point_groups.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "expressions",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_expressions,
        len: |m| m.om.expressions.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "class_definitions",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_classes,
        len: |m| m.om.classes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "field_definitions",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_fields,
        len: |m| m.om.fields.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "object_records",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_object_records,
        len: |m| m.om.object_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "rmfastload_object_id_tables",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_rmfastload_object_id_tables,
        len: |m| m.om.rmfastload_object_id_tables.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "rmfastload_object_ids",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_rmfastload_object_ids,
        len: |m| m.om.rmfastload_object_ids.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_blocks",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_data_blocks,
        len: |m| m.om.data_blocks.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_counted_index_lanes",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_data_block_counted_index_lanes,
        len: |m| m.om.data_block_counted_index_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_index_rows",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_data_block_index_rows,
        len: |m| m.om.data_block_index_rows.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "data_block_linked_index_rows",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_data_block_linked_index_rows,
        len: |m| m.om.data_block_linked_index_rows.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "data_block_target_index_rows",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_data_block_target_index_rows,
        len: |m| m.om.data_block_target_index_rows.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "data_block_column_index_tables",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_data_block_column_index_tables,
        len: |m| m.om.data_block_column_index_tables.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_input_column_row_uses",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_input_column_row_uses,
        len: |m| m.features.feature_input_column_row_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_input_column_targets",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_features_feature_input_column_targets,
        len: |m| m.features.feature_input_column_targets.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "string_values",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_string_values,
        len: |m| m.om.string_values.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "object_references",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_object_references,
        len: |m| m.om.object_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "persistent_handles",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_persistent_handles,
        len: |m| m.om.persistent_handles.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "configurations",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_configurations,
        len: |m| m.om.configurations.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "configuration_attribute_uses",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_configuration_attribute_uses,
        len: |m| m.om.configuration_attribute_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "part_attributes",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_part_attributes,
        len: |m| m.om.part_attributes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "external_reference_indexed_records",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_external_reference_indexed_records,
        len: |m| m.om.external_reference_indexed_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "external_reference_empty_records",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_external_reference_empty_records,
        len: |m| m.om.external_reference_empty_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "external_reference_tail_reference_pairs",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_external_reference_tail_reference_pairs,
        len: |m| m.om.external_reference_tail_reference_pairs.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "external_reference_record_string_uses",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_external_reference_record_string_uses,
        len: |m| m.om.external_reference_record_string_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "external_reference_record_children",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: emit_om_external_reference_record_children,
        len: |m| m.om.external_reference_record_children.len(),
        counts_toward_emptiness: true,
    },
];
