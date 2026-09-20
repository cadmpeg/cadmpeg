// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_test_support::EditableDecodeResult;

use crate::container::SectionRole;
use crate::test_support::assert_annotation;
use crate::test_support::build_prt;
use crate::test_support::build_prt_raw;
use crate::test_support::jpeg_payload;
use crate::test_support::visibgeom_payload;

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};
use cadmpeg_ir::Exactness;

use crate::container::{self, Layout, UnknownLayout};
use crate::CreoCodec;

#[test]
fn detect_matches_ugc_magic_only() {
    let codec = CreoCodec;
    assert_eq!(codec.detect(b"#UGC:2 P foo"), Confidence::High);
    // A Siemens NX `.prt` (shares the extension) must not be claimed here.
    assert_eq!(codec.detect(b"\x0e\x93\x13\x01NX"), Confidence::No);
    assert_eq!(codec.detect(b"PK\x03\x04"), Confidence::No);
    assert_eq!(codec.detect(b""), Confidence::No);
}

#[test]
fn scan_decodes_length_prefixed_native_model_name() {
    let data = b"#UGC:2 PART test \\\n#- CMNM 00bwidget.prt                                      \\\n#-END_OF_UGC_HEADER\n"
        .to_vec();
    let scan = container::scan_bytes_ok(data.clone());

    assert_eq!(
        scan.framing
            .model_name
            .as_ref()
            .map(|model| model.name.as_str()),
        Some("widget.prt ")
    );
    let model_name_offset = data
        .windows(b"widget.prt ".len())
        .position(|window| window == b"widget.prt ")
        .expect("model name offset");
    assert_eq!(
        scan.framing.model_name.as_ref().map(|model| model.offset),
        Some(model_name_offset)
    );
    let result = EditableDecodeResult::from(
        CreoCodec
            .decode(&mut Cursor::new(data), &DecodeOptions::default())
            .expect("decode"),
    );
    assert_eq!(
        result
            .ir()
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("model_name"))
            .map(String::as_str),
        Some("widget.prt ")
    );
    let [product] = result.ir().model.product_definitions.as_slice() else {
        panic!("one part product");
    };
    assert_eq!(product.id.as_str(), "creo:model:product_definition#root");
    assert_eq!(product.part_number.as_deref(), Some("widget.prt "));
    assert_eq!(product.label.as_deref(), Some("widget.prt "));
    assert!(product.bodies.is_empty());
    let [occurrence] = result.ir().model.occurrences.as_slice() else {
        panic!("one root occurrence");
    };
    assert!(matches!(
        &occurrence.prototype,
        cadmpeg_ir::products::PrototypeReference::Local { definition }
            if definition == &product.id
    ));
    assert!(matches!(
        occurrence.parent,
        cadmpeg_ir::products::OccurrenceParent::Root {}
    ));
    assert_eq!(
        occurrence.transform,
        cadmpeg_ir::transform::Transform::identity()
    );
    assert_annotation(
        &result.source_fidelity().annotations,
        product.id.as_str(),
        "creo:archive_header",
        model_name_offset as u64,
        "part_product",
        Exactness::Derived,
    );
    assert_annotation(
        &result.source_fidelity().annotations,
        occurrence.id.as_str(),
        "creo:archive_header",
        model_name_offset as u64,
        "part_product_occurrence",
        Exactness::Derived,
    );
}

#[test]
fn scan_withholds_repeated_native_model_names() {
    let data = b"#UGC:2 PART test \\\n+#- CMNM 00awidget.prt                                      \\\n+#- CMNM 00bwidget2.prt                                     \\\n+#-END_OF_UGC_HEADER\n"
        .to_vec();

    let scan = container::scan_bytes_ok(data);
    assert!(scan.framing.model_name.is_none());
}

#[test]
fn scan_decodes_binary_model_name_field_without_cmnm_header() {
    let data = build_prt(
        "test",
        &[(
            "BasicData",
            b"e0\x0amodel_name\0\xf1WIDGET_ROOT\0e0\x00disp_outl_info\0".to_vec(),
        )],
    );

    let scan = container::scan_bytes_ok(data.clone());
    assert_eq!(
        scan.framing
            .model_name
            .as_ref()
            .map(|model| model.name.as_str()),
        Some("WIDGET_ROOT")
    );
    let model_name_offset = data
        .windows(b"WIDGET_ROOT".len())
        .position(|window| window == b"WIDGET_ROOT")
        .expect("model name offset");
    assert_eq!(
        scan.framing.model_name.as_ref().map(|model| model.offset),
        Some(model_name_offset)
    );

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    assert_eq!(
        result
            .ir()
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("model_name"))
            .map(String::as_str),
        Some("WIDGET_ROOT")
    );
}

#[test]
fn scan_skips_empty_binary_model_name_fields() {
    let data = build_prt(
        "test",
        &[(
            "BasicData",
            b"e0\x0amodel_name\0\xe1e0\x0amodel_name\0ROOT\0".to_vec(),
        )],
    );

    let scan = container::scan_bytes_ok(data);
    assert_eq!(
        scan.framing
            .model_name
            .as_ref()
            .map(|model| model.name.as_str()),
        Some("ROOT")
    );
}

#[test]
fn relation_model_name_accepts_binary_root_name() {
    assert_eq!(
        super::relation_model_name("DRILL_BIT_10D0_SUPPRESSED_FEAT"),
        Some("DRILL_BIT_10D0_SUPPRESSED_FEAT")
    );
    assert_eq!(super::relation_model_name("widget.PrT "), Some("widget"));
    assert_eq!(super::relation_model_name("widget.step"), None);
}

#[test]
fn scan_enumerates_and_classifies_sections() {
    let data = build_prt(
        "test",
        &[
            ("VisibGeom", visibgeom_payload(5, 12)),
            ("AllFeatur", vec![0x01, 0x02, 0x03]),
            ("THMB_IMG_MAIN", jpeg_payload()),
        ],
    );
    let scan = container::scan_bytes_ok(data);

    assert_eq!(scan.framing.version_line, "#UGC:2 P test");
    assert_eq!(scan.framing.sections.len(), 3);
    assert_eq!(scan.framing.sections[0].name(), "VisibGeom");
    assert_eq!(scan.framing.sections[0].role(), SectionRole::PsbGeometry);
    assert_eq!(scan.framing.sections[1].name(), "AllFeatur");
    assert_eq!(scan.framing.sections[1].role(), SectionRole::ModelData);
    assert_eq!(scan.framing.sections[2].role(), SectionRole::Thumbnail);
    assert!(container::has_thumbnail(&scan));
}

#[test]
fn scan_finds_curve_expression_in_feature_definition_section() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01value=5\0"
        .to_vec();
    let scan = container::scan_bytes_ok(build_prt("c", &[("FeatDefs", payload)]));

    assert_eq!(scan.curves.expressions.len(), 1);
    assert_eq!(scan.curves.expressions[0].entity_id, 7);
    assert_eq!(scan.curves.expressions[0].lines[0].text, "value=5");
}

#[test]
fn scan_enumerates_toc_backed_compound_close_section_boundaries() {
    let mut data = b"#UGC:2 P test\n#-END_OF_UGC_HEADER\n#UGC_TOC\n\
        DEPDB_DATA 1 2 3\nVisibGeom 4 5 6\nAllFeatur 7 8 9\n\
        #END_OF_TOC_HEADER\n#DEPDB_DATA\nopaque"
        .to_vec();
    data.extend_from_slice(b"\xf1#VisibGeom\npacked\xf1#not_in_toc\ninside");
    data.extend_from_slice(b"\xf1#AllFeatur\nfeatures");

    let scan = container::scan_bytes_ok(data);

    assert_eq!(
        scan.framing
            .sections
            .iter()
            .map(super::Section::name)
            .collect::<Vec<_>>(),
        ["DEPDB_DATA", "VisibGeom", "AllFeatur"]
    );
    assert_eq!(scan.framing.sections[1].role(), SectionRole::PsbGeometry);
    assert_eq!(scan.framing.sections[2].role(), SectionRole::ModelData);
}

#[test]
fn scan_uses_fixed_width_toc_offsets_for_adjacent_sections() {
    let mut data = b"#UGC:2 P test\n#-END_OF_UGC_HEADER\n".to_vec();
    let header_base = data.len();
    data.extend_from_slice(format!("{:<80}\n", "#UGC_TOC 2 2 81 17").as_bytes());
    let first_offset = 3 * 81;
    let first = b"#SolidPrimdata\nabc";
    let second_offset = first_offset + first.len();
    let second = b"#VisibGeom\nxyz";
    data.extend_from_slice(
        format!(
            "{:<80}\n",
            format!("SolidPrimdata {first_offset:x} {:x} 0", first.len())
        )
        .as_bytes(),
    );
    data.extend_from_slice(
        format!(
            "{:<80}\n",
            format!("VisibGeom {second_offset:x} {:x} 0", second.len())
        )
        .as_bytes(),
    );
    assert_eq!(data.len(), header_base + first_offset);
    data.extend_from_slice(first);
    data.extend_from_slice(second);

    let scan = container::scan_bytes_ok(data);

    assert_eq!(scan.framing.sections.len(), 2);
    assert_eq!(scan.framing.sections[0].name(), "SolidPrimdata");
    assert_eq!(scan.framing.sections[0].length, first.len());
    assert_eq!(scan.framing.sections[1].name(), "VisibGeom");
    assert_eq!(scan.framing.sections[1].offset, header_base + second_offset);
}

#[test]
fn scan_expands_toc_sized_unix_compress_payload() {
    let mut data = b"#UGC:2 P test\n#-END_OF_UGC_HEADER\n".to_vec();
    let header_base = data.len();
    data.extend_from_slice(format!("{:<80}\n", "#UGC_TOC 2 1 81 17").as_bytes());
    let section_offset = 2 * 81;
    let compressed = [0x1f, 0x9d, 0x10, 0x41, 0x84, 0x0c, 0x01];
    let section_length = b"#SolidPrimdata\n".len() + compressed.len();
    data.extend_from_slice(
        format!(
            "{:<80}\n",
            format!("SolidPrimdata {section_offset:x} {section_length:x} 3")
        )
        .as_bytes(),
    );
    assert_eq!(data.len(), header_base + section_offset);
    data.extend_from_slice(b"#SolidPrimdata\n");
    data.extend_from_slice(&compressed);

    let scan = container::scan_bytes_ok(data);
    let classification = crate::dialect::classify(&scan);

    assert_eq!(scan.framing.expanded_sections.len(), 1);
    assert_eq!(scan.framing.expanded_sections[0].data, b"ABC");
    let summary = container::summarize(&scan, &classification);
    let cadmpeg_core::container::EntryStorage::Compressed {
        method,
        stored,
        expanded,
    } = summary.entries[0].storage
    else {
        panic!("a unix-compressed section stores compressed bytes");
    };
    assert_eq!(
        method,
        cadmpeg_core::container::CompressionMethod::UnixCompress
    );
    assert_eq!(stored, Some(section_length as u64));
    assert_eq!(expanded, Some(18));
}

#[test]
fn scan_reads_namespace_counts() {
    let data = build_prt("c", &[("VisibGeom", visibgeom_payload(5, 12))]);
    let scan = container::scan_bytes_ok(data);
    assert_eq!(scan.framing.census.srf_array_count, Some(5));
    assert_eq!(scan.framing.census.crv_array_count, Some(12));
}

#[test]
fn scan_sums_concatenated_depdb_surface_namespaces() {
    let mut payload = visibgeom_payload(3, 4);
    payload.extend_from_slice(&visibgeom_payload(5, 6));
    let scan = container::scan_bytes_ok(build_prt("c", &[("DEPDB_DATA", payload)]));

    assert_eq!(scan.framing.layout, Layout::Depdb);
    assert_eq!(scan.framing.census.srf_array_count, Some(8));
    assert_eq!(scan.framing.census.crv_array_count, Some(10));
}

#[test]
fn scan_does_not_treat_unlabeled_depdb_bytes_as_geometry_rows() {
    let payload = vec![7, 0x22, 4, 0x01, 0, 8, 8, 0x24, 4, 0xf6, 0x01, 0];
    let scan = container::scan_bytes_ok(build_prt("c", &[("DEPDB_DATA", payload)]));

    assert!(scan.surfaces.rows.is_empty());
    assert!(scan.surfaces.parameters.is_empty());
}

#[test]
fn scan_reads_declared_geomlists_body_count() {
    let scan = container::scan_bytes_ok(build_prt(
        "c",
        &[("Geomlists", b"n_bodies\0\x83\x01".to_vec())],
    ));

    assert_eq!(scan.framing.declared_body_count, Some(769));
}

#[test]
fn scan_reads_geomlists_first_quilt_discriminator() {
    let scan = container::scan_bytes_ok(build_prt(
        "c",
        &[("Geomlists", b"first_quilt_ptr\0\x00".to_vec())],
    ));

    assert_eq!(scan.framing.first_quilt_ptr, Some(0));
}

#[test]
fn scan_reads_legacy_geom_depend_first_quilt_discriminator() {
    let data = b"#UGC:2 PART c\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n\
@Sld_GeomDepend 1 0\n0 1 ->\n\
@first_quilt_ptr 4 1\n1 4 0\n\
#END_OF_P_OBJECT\n#Pro/ENGINEER  TM  Version H-01-21\n"
        .to_vec();

    let scan = container::scan_bytes_ok(data);

    assert!(matches!(scan.framing.layout, Layout::LegacyAscii(_)));
    assert_eq!(scan.framing.first_quilt_ptr, Some(0));
}

#[test]
fn legacy_geom_depend_discriminator_withholds_distinct_values() {
    let root = crate::legacy::ObjectRecord {
        name: "Sld_GeomDepend".to_string(),
        attribute_id: 1,
        scope_offset: 0,
        parent: None,
        depth: 0,
        payload: crate::legacy::ObjectPayload::Arrow,
        offset: 0,
    };
    let persistence = crate::legacy::Persistence {
        objects: vec![root],
        integer_values: crate::legacy::TypedValues {
            rows: vec![
                crate::legacy::IntegerRecord {
                    name: "first_quilt_ptr".to_string(),
                    attribute_id: 4,
                    scope_offset: 0,
                    parent: Some(0),
                    depth: 1,
                    payload: crate::legacy::NumericPayload::Scalar { value: 0 },
                    offset: 1,
                },
                crate::legacy::IntegerRecord {
                    name: "first_quilt_ptr".to_string(),
                    attribute_id: 4,
                    scope_offset: 0,
                    parent: Some(0),
                    depth: 1,
                    payload: crate::legacy::NumericPayload::Scalar { value: 7 },
                    offset: 2,
                },
            ],
            unresolved_count: 0,
        },
        ..Default::default()
    };

    assert_eq!(
        super::legacy_geom_depend_value(&persistence, "first_quilt_ptr"),
        None
    );
}

#[test]
fn nd_decoration_selects_nd_layout() {
    let data = build_prt("c", &[("ND:0:VisibGeom:1", visibgeom_payload(3, 4))]);
    let scan = container::scan_bytes_ok(data);
    assert_eq!(scan.framing.layout, Layout::Nd);
    // The decorated name is normalized for classification and census.
    assert_eq!(scan.framing.sections[0].name(), "VisibGeom");
    assert_eq!(scan.framing.sections[0].raw_name, "ND:0:VisibGeom:1");
    assert_eq!(scan.framing.census.srf_array_count, Some(3));
}

#[test]
fn depdb_root_record_overrides_embedded_nd_decoration() {
    let data = build_prt(
        "c",
        &[
            ("DEPDB_DATA", b"\xe0\x00p_dep_db\0\xe3".to_vec()),
            ("ND:0:Model_L05P:1", Vec::new()),
        ],
    );
    let scan = container::scan_bytes_ok(data);
    assert_eq!(scan.framing.layout, Layout::Depdb);
}

#[test]
fn depdb_layout_requires_root_record() {
    let data = build_prt_raw(
        "c",
        &[
            ("DEPDB_DATA", b"not-a-root".to_vec()),
            ("ND:0:Model_L05P:1", Vec::new()),
        ],
    );
    let scan = container::scan_bytes_ok(data);
    assert_eq!(
        scan.framing.layout,
        Layout::Unknown(UnknownLayout::DepdbRootMissing)
    );
}

#[test]
fn visible_geometry_namespace_excludes_invisible_and_depdb_rows() {
    let mut visible = visibgeom_payload(1, 0);
    visible.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0, 0xe4, 0xe3]);
    visible.extend_from_slice(b"crv_array\0crv_id\0\x07type\0\x08feat_id\0\x04");
    visible
        .extend_from_slice(b"topol_ref_data\0\x07\x08\x04\x01\xf6\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3");
    let mut invisible = visibgeom_payload(1, 0);
    invisible.extend_from_slice(&[8, 0x26, 5, 0x01, 0, 0, 0xe4, 0xe3]);
    invisible.extend_from_slice(b"srf_prim_ptr(cylinder)\0\xe0\x01radius\0\xe4");
    invisible.extend_from_slice(b"crv_array\0crv_id\0\x07type\0\x09feat_id\0\x05");
    invisible
        .extend_from_slice(b"topol_ref_data\0\x07\x09\x05\x01\xf6\x0c\x0d\x07\x07\0\0\xe3\xe1\xe3");
    let mut depdb = visibgeom_payload(1, 0);
    depdb.extend_from_slice(&[9, 0x26, 6, 0x01, 0, 0, 0xe4, 0xe3]);

    let scan = container::scan_bytes_ok(build_prt(
        "c",
        &[
            ("VisibGeom", visible),
            ("NovisGeom", invisible),
            ("DEPDB_DATA", depdb),
        ],
    ));

    assert_eq!(
        scan.surfaces
            .rows
            .iter()
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        [7]
    );
    assert_eq!(
        scan.surfaces
            .parameters
            .iter()
            .map(|record| record.surface_id)
            .collect::<Vec<_>>(),
        [7]
    );
    assert_eq!(
        scan.surfaces
            .nonvisible_rows
            .iter()
            .map(|row| (row.id, row.feature_id))
            .collect::<Vec<_>>(),
        [(8, 5)]
    );
    assert_eq!(scan.curves.prototypes.len(), 1);
    assert_eq!(scan.surfaces.nonvisible_parameters.len(), 1);
    assert_eq!(scan.surfaces.nonvisible_parameters[0].surface_id, 8);
    assert_eq!(
        scan.surfaces.nonvisible_parameters[0].scalar_values(),
        [1.0]
    );
    assert_eq!(scan.surfaces.nonvisible_prototype_records.len(), 1);
    assert_eq!(
        scan.surfaces.nonvisible_prototype_records[0].family.name(),
        "cylinder"
    );
    assert_eq!(scan.curves.nonvisible_prototypes.len(), 1);
    assert_eq!(scan.curves.nonvisible_prototypes[0].feature_id, Some(5));
    assert_eq!(scan.curves.parameters.len(), 1);
    assert_eq!(scan.curves.nonvisible_parameters.len(), 1);
    assert_eq!(
        scan.curves.topology_rows[0].faces,
        [std::num::NonZeroU32::new(10), std::num::NonZeroU32::new(11)]
    );
    assert_eq!(
        scan.curves.nonvisible_topology_rows[0].faces,
        [std::num::NonZeroU32::new(12), std::num::NonZeroU32::new(13)]
    );
    assert_eq!(scan.topology.half_edges.len(), 2);

    let result = CreoCodec
        .decode(
            &mut Cursor::new(scan.framing.data.clone()),
            &DecodeOptions::default(),
        )
        .expect("decode");
    let rows = &result.ir().native.namespace("creo").unwrap().arenas()["nonvisible_surface_rows"];
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id(), "creo:novisgeom:surface_row#8");
    assert_eq!(rows[0].fields()["source_section"], "NovisGeom");
    let namespace = result.ir().native.namespace("creo").unwrap();
    let surface_parameters = &namespace.arenas()["nonvisible_surface_parameters"];
    assert_eq!(
        surface_parameters[0].id(),
        "creo:novisgeom:surface_parameter#8"
    );
    assert_eq!(surface_parameters[0].fields()["slots"][0]["value"], 1.0);
    let surface_prototypes = &namespace.arenas()["nonvisible_surface_prototypes"];
    assert!(surface_prototypes[0]
        .id()
        .starts_with("creo:novisgeom:surface_prototype#"));
    assert_eq!(
        surface_prototypes[0].fields()["source_section"],
        "NovisGeom"
    );
    let prototypes = &namespace.arenas()["nonvisible_curve_prototypes"];
    assert_eq!(prototypes[0].fields()["curve_id"], 7);
    assert_eq!(prototypes[0].fields()["source_section"], "NovisGeom");
    let parameters = &namespace.arenas()["nonvisible_curve_parameters"];
    assert_eq!(parameters[0].id(), "creo:novisgeom:curve_parameter#7");
    let topology = &namespace.arenas()["nonvisible_curve_topology_rows"];
    assert_eq!(topology[0].id(), "creo:novisgeom:curve_topology#7");
    assert_eq!(topology[0].fields()["faces"][0], 12);
}

#[test]
fn depdb_data_with_sparse_sections_selects_depdb() {
    let depdb = b"srf_array\0geom_id\0\x07geom_type\0\x22feat_id\0\x04orient\0\x01boundary_type\0\0next_geom_ptr\0\0feat_defs_12\0protrevolve\0Revolve id 17\0".to_vec();
    let data = build_prt("c", &[("VisibGeom", vec![0x00]), ("DEPDB_DATA", depdb)]);
    let scan = container::scan_bytes_ok(data);
    assert_eq!(scan.framing.layout, Layout::Depdb);
    assert!(scan
        .surfaces
        .rows
        .iter()
        .any(|row| row.id == 7 && row.feature_id == 4));
    assert!(scan
        .features
        .definitions
        .iter()
        .any(|definition| definition.identity.id() == 12));
    assert_eq!(scan.features.operations.len(), 1);
    assert_eq!(scan.features.operations[0].feature_id, 17);
    assert_eq!(
        scan.features.operations[0].recipe.resolved(),
        Some(crate::feature::operations::FeatureRecipe::ProtrudeRevolve)
    );
}

#[test]
fn framing_names_are_not_mistaken_for_sections() {
    let data = build_prt("c", &[("VisibGeom", vec![0x00])]);
    let scan = container::scan_bytes_ok(data);
    // Only VisibGeom — the header/TOC framing markers are excluded.
    assert_eq!(scan.framing.sections.len(), 1);
    assert_eq!(scan.framing.sections[0].name(), "VisibGeom");
}

#[test]
fn inspect_summary_has_layout_and_census_notes() {
    let data = build_prt("c", &[("ND:0:VisibGeom:1", visibgeom_payload(7, 9))]);
    let mut reader = Cursor::new(data);
    let summary = CreoCodec
        .inspect(
            &mut reader,
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .expect("inspect");
    assert_eq!(summary.format(), "creo");
    assert_eq!(summary.container_kind, "psb");
    assert!(summary.notes.iter().any(|n| n.contains("layout: ND")));
    assert!(summary.notes.iter().any(|n| n.contains("srf_array=7")));
}

#[test]
fn a_section_extent_the_file_does_not_hold_is_not_a_section() {
    let data = b"#Geomlists\n0123";

    // One byte past the last byte of the file.
    assert!(
        container::Section::scan("Geomlists".to_string(), 0, data.len() + 1, None, data).is_none()
    );

    // An offset and a length that state no address between them: the end is
    // before the offset, which is what an overflowing `offset + length` leaves.
    assert!(
        container::Section::scan("Geomlists".to_string(), usize::MAX - 3, 12, None, data).is_none()
    );
}

#[test]
fn a_section_that_ends_on_the_last_byte_is_admitted() {
    let data = b"#Geomlists\n0123";
    let section = container::Section::scan("Geomlists".to_string(), 0, data.len(), None, data)
        .expect("section extent")
        .section;

    assert_eq!(section.offset(), 0);
    assert_eq!(section.length(), data.len());
    assert_eq!(section.end(), data.len());
    assert_eq!(
        container::section_region(data, &section).expect("the section is a region of `data`"),
        data.as_slice()
    );
}

/// Inside the scan a section carries the bytes it was admitted against, so a
/// reader reads them with no second bound and no `Option`. The reader below
/// takes `ScannedSection`; the owned `Section` carries no region and does not
/// compile in its place.
#[test]
fn an_in_scan_reader_reads_the_region_its_section_was_admitted_against() {
    let data = b"#VisibGeom\nsrf_array\0";
    let scanned = container::Section::scan("VisibGeom".to_string(), 0, data.len(), None, data)
        .expect("section extent");
    assert_eq!(scanned.region, data.as_slice());

    let selected = super::model_geometry_sections(std::slice::from_ref(&scanned));
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].section.name(), "VisibGeom");
    assert_eq!(selected[0].region, data.as_slice());
}

#[test]
fn a_section_contains_its_own_offset_and_every_byte_before_its_end() {
    let data = b"0123#Geomlists\n0123";
    let section = container::Section::scan("Geomlists".to_string(), 4, data.len(), None, data)
        .expect("section extent")
        .section;

    assert!(!section.contains(section.offset() - 1));
    assert!(section.contains(section.offset()));
    assert!(section.contains(section.end() - 1));
    assert!(!section.contains(section.end()));
}

#[test]
fn a_section_region_is_exactly_the_bytes_between_the_section_offset_and_its_end() {
    let data = build_prt(
        "test",
        &[
            ("Geomlists", b"0123".to_vec()),
            ("Xsections", b"ABCD".to_vec()),
        ],
    );
    let scan = container::scan_bytes_ok(data.clone());
    let section = scan
        .framing
        .sections
        .iter()
        .find(|section| section.name() == "Xsections")
        .expect("the scan enumerates the second section");

    // The region is read through the scan that proved it, so it is the file's
    // own bytes over the section's own extent, with no refusal in between.
    assert_eq!(
        container::section_region(&scan.framing.data, section)
            .expect("the scan enumerated the section over this file"),
        &data[section.offset()..section.end()]
    );
    assert!(container::section_region(&scan.framing.data, section)
        .expect("the scan enumerated the section over this file")
        .starts_with(b"#Xsections\nABCD"));
}

/// `section_region` answers `None` on a slice the section's extent leaves. The
/// bound is the slice's own, so this is a statement about the function, not
/// about a decode route: no production caller passes anything but the scanned
/// file.
#[test]
fn a_section_region_is_absent_from_a_slice_shorter_than_the_section() {
    let data = build_prt(
        "test",
        &[
            ("Geomlists", b"0123".to_vec()),
            ("Xsections", b"ABCD".to_vec()),
        ],
    );
    let scan = container::scan_bytes_ok(data.clone());
    let section = scan
        .framing
        .sections
        .iter()
        .find(|section| section.name() == "Xsections")
        .expect("the scan enumerates the second section");

    let truncated = &data[..section.end() - 1];
    assert_eq!(container::section_region(truncated, section), None);
    assert_eq!(
        container::section_region(&data[..section.end()], section),
        Some(&data[section.offset()..section.end()])
    );
}
