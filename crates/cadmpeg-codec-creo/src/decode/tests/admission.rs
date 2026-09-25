// SPDX-License-Identifier: Apache-2.0
//! Synthetic byte-literal tests for the container framing and honest decode.
//!
//! No external CAD file is used; every fixture is a hand-built PSB byte image
//! exercising the `#UGC:2` framing, the `#\n#<name>\n` section-boundary rule, the
//! persistence-layout signals, and the `srf_array`/`crv_array` count headers.
#![allow(clippy::unwrap_used)]
use cadmpeg_test_support::{wire, EditableDecodeResult};

use crate::test_support::allfeatur_row;
use crate::test_support::assert_annotation;
use crate::test_support::build_prt;
use crate::test_support::build_toc_section_prt;
use crate::test_support::jpeg_payload;
use crate::test_support::unix_compress_literals;
use crate::test_support::visibgeom_payload;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;

use cadmpeg_core::container::ContainerRole;

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_ir::Exactness;

use crate::container::{self};
use crate::CreoCodec;

#[test]
fn decode_refuses_when_max_entities_is_below_section_cardinality() {
    use cadmpeg_core::decode::ResourceDimension;

    let data = build_prt(
        "c",
        &[
            ("THMB_IMG_MAIN", jpeg_payload()),
            ("VisibGeom", visibgeom_payload(0, 0)),
        ],
    );
    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = 1;
    let error = CreoCodec
        .decode(&mut Cursor::new(data), &options)
        .expect_err("max_entities below section count must refuse at admission");
    assert!(
        matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
        ),
        "{error:?}"
    );
}

#[test]
fn decode_keeps_section_and_model_entity_admission_additive() {
    use cadmpeg_core::decode::ResourceDimension;

    let fixture = include_bytes!("../../../tests/golden/fixtures/named_cylinder_prototype.prt");
    let decoded = CreoCodec
        .decode(&mut Cursor::new(fixture), &DecodeOptions::default())
        .expect("decode named cylinder prototype");
    assert_eq!(
        container::scan_bytes_ok(fixture.as_slice())
            .framing
            .sections
            .len(),
        1
    );
    assert_eq!(decoded.ir().model.entity_count(), 2);

    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = 2;
    let error = CreoCodec
        .decode(&mut Cursor::new(fixture), &options)
        .expect_err("one section plus two model entities require an entity limit of three");
    assert!(
        matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "admit Creo model features"
        ),
        "{error:?}"
    );

    options.policy.limits.max_entities = 3;
    CreoCodec
        .decode(&mut Cursor::new(fixture), &options)
        .expect("the exact additive entity limit must admit the fixture");
}

#[test]
fn part_product_occurrence_refuses_before_model_insertion() {
    use cadmpeg_core::decode::ResourceDimension;

    let data = include_bytes!("../../../tests/golden/fixtures/native_model_name.prt");
    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = 1;
    let error = CreoCodec
        .decode(&mut Cursor::new(data), &options)
        .expect_err("a product definition and occurrence require two entity slots");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "admit Creo model occurrences"
    ));

    options.policy.limits.max_entities = 2;
    let decoded = CreoCodec
        .decode(&mut Cursor::new(data), &options)
        .expect("the exact model entity limit admits the part product");
    assert_eq!(decoded.ir().model.product_definitions.len(), 1);
    assert_eq!(decoded.ir().model.occurrences.len(), 1);
}

fn sketch_admission_prt() -> Vec<u8> {
    let mut definition =
        b"feat_defs_40\0segtab_ptr\0\xf8\x02\xf7\x01\xfb\xe2schema\xf2\xf7\x01\xe2".to_vec();
    definition.extend_from_slice(&[2, 0, 0, 0, 7, 8, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2, 0xe3]);
    definition.extend_from_slice(&[25, 0, 0, 0, 8, 9, 0xf6, 0, 0, 0xf6, 0xf6, 42, 0xe2, 0xe3]);
    definition.extend_from_slice(
        b"dimtab_ptr\0\xf8\x01\xf7\x58\xfb\xe2\
          \xe0\x01type\0\x02\xe0\x01value\0\xe4\
          \xe0\x01direct\0\x00\xe0\x01aux_value\0\x0f\
          \xe0\x01ext_id\0\x2a\xe0\x00relat_ptr\0",
    );
    build_prt("c", &[("FeatDefs", definition)])
}

#[test]
fn sketch_entity_refuses_before_model_insertion() {
    use cadmpeg_core::decode::ResourceDimension;

    let data = sketch_admission_prt();
    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = 1;
    let error = CreoCodec
        .decode(&mut Cursor::new(data.clone()), &options)
        .expect_err("one section exhausts the entity limit before sketch entities");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "admit Creo model sketch_entities"
    ));

    let decoded = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("service profile admits the sketch");
    assert_eq!(decoded.ir().model.sketch_entities.len(), 2);
}

#[test]
fn sketch_constraint_refuses_before_model_insertion() {
    use cadmpeg_core::decode::ResourceDimension;

    let data = sketch_admission_prt();
    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = 3;
    let error = CreoCodec
        .decode(&mut Cursor::new(data.clone()), &options)
        .expect_err("the section and two sketch entities exhaust the entity limit");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "admit Creo model sketch_constraints"
    ));

    let decoded = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("service profile admits the sketch constraints");
    assert_eq!(decoded.ir().model.sketch_constraints.len(), 2);

    let mut exact = DecodeOptions::default();
    exact.policy.limits.max_entities =
        u64::try_from(1 + decoded.ir().model.entity_count()).expect("fixture count fits u64");
    CreoCodec
        .decode(&mut Cursor::new(sketch_admission_prt()), &exact)
        .expect("section and each model identity are charged once");
}

#[test]
fn decode_extracts_jpeg_thumbnail_as_native_asset() {
    let data = build_prt("c", &[("THMB_IMG_MAIN", jpeg_payload())]);
    let result = EditableDecodeResult::from(
        CreoCodec
            .decode(
                &mut Cursor::new(data),
                &DecodeOptions {
                    container_only: true,
                    ..DecodeOptions::default()
                },
            )
            .expect("decode thumbnail"),
    );

    assert!(!result.report().geometry_transferred());
    let unknowns = result.ir().native_unknowns("creo").unwrap();
    assert_eq!(unknowns.len(), 1);
    let retained = result
        .source_fidelity()
        .retained_record(unknowns[0].id.as_str())
        .expect("retained thumbnail");
    assert_eq!(retained.data(), Some(jpeg_payload().as_slice()));
    assert_annotation(
        &result.source_fidelity().annotations,
        unknowns[0].id.as_str(),
        "creo:THMB_IMG_MAIN",
        retained.offset(),
        "jpeg_thumbnail",
        Exactness::ByteExact,
    );
    let source = result.ir().source.as_ref().expect("source metadata");
    assert_eq!(source.attributes["section_count"], "1");
    assert_eq!(source.attributes["section.0.name"], "THMB_IMG_MAIN");
    assert_eq!(source.attributes["section.0.raw_name"], "THMB_IMG_MAIN");
    assert_eq!(
        source.attributes["section.0.role"],
        ContainerRole::Thumbnail.as_str()
    );
    assert!(source.attributes["section.0.offset"]
        .parse::<usize>()
        .is_ok());
    assert!(source.attributes["section.0.length"]
        .parse::<usize>()
        .is_ok());
}

#[test]
fn thumbnail_passthrough_copy_refuses_on_retained_byte_limit() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    let jpeg = jpeg_payload();
    let data = build_prt("c", &[("THMB_IMG_MAIN", jpeg.clone())]);
    let mut options = DecodeOptions {
        container_only: true,
        policy: DecodePolicy::service(),
    };
    options.policy.limits.max_retained_bytes =
        u64::try_from(jpeg.len() - 1).expect("fixture length fits the resource limit");
    let error = CreoCodec
        .decode(&mut Cursor::new(data.clone()), &options)
        .expect_err("thumbnail copy exceeds the retained-byte limit");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Creo passthrough section"
    ));

    options.policy.limits.max_retained_bytes =
        u64::try_from(jpeg.len()).expect("fixture length fits the resource limit");
    CreoCodec
        .decode(&mut Cursor::new(data), &options)
        .expect("the exact retained-byte limit admits the thumbnail");
}

#[test]
fn geometry_passthrough_copy_refuses_on_retained_byte_limit() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    let geometry = visibgeom_payload(0, 0);
    let data = build_prt("c", &[("VisibGeom", geometry)]);
    let section_len = container::scan_bytes_ok(data.clone()).framing.sections[0].length();
    let mut options = DecodeOptions {
        container_only: true,
        policy: DecodePolicy::service(),
    };
    options.policy.limits.max_retained_bytes =
        u64::try_from(section_len - 1).expect("fixture length fits the resource limit");
    let error = CreoCodec
        .decode(&mut Cursor::new(data.clone()), &options)
        .expect_err("geometry copy exceeds the retained-byte limit");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Creo passthrough section"
    ));

    options.policy.limits.max_retained_bytes =
        u64::try_from(section_len).expect("fixture length fits the resource limit");
    CreoCodec
        .decode(&mut Cursor::new(data), &options)
        .expect("the exact retained-byte limit admits the geometry section");
}

#[test]
fn decode_expands_and_retains_compressed_jpeg_thumbnail() {
    let jpeg = jpeg_payload();
    let compressed = unix_compress_literals(&jpeg);
    let data = build_toc_section_prt("THMB_IMG_MAIN", &compressed, jpeg.len());
    let scan = container::scan_bytes_ok(data.clone());

    assert_eq!(scan.framing.expanded_sections.len(), 1);
    assert_eq!(scan.framing.expanded_sections[0].data, jpeg);
    assert!(container::has_thumbnail(&scan));
    let classification = crate::dialect::classify(&scan);
    assert!(container::summarize(&scan, &classification)
        .notes
        .iter()
        .any(|note| note.contains("THMB_IMG_MAIN carries a JPEG preview")));

    let source_offset = scan.framing.expanded_sections[0].source_offset;
    let result = EditableDecodeResult::from(
        CreoCodec
            .decode(
                &mut Cursor::new(data),
                &DecodeOptions {
                    container_only: true,
                    ..DecodeOptions::default()
                },
            )
            .expect("decode compressed thumbnail"),
    );
    let unknowns = result.ir().native_unknowns("creo").unwrap();
    assert_eq!(unknowns.len(), 1);
    let retained = result
        .source_fidelity()
        .retained_record(unknowns[0].id.as_str())
        .expect("retained expanded thumbnail");
    assert_eq!(retained.data(), Some(jpeg.as_slice()));
    assert_annotation(
        &result.source_fidelity().annotations,
        unknowns[0].id.as_str(),
        "creo:THMB_IMG_MAIN",
        source_offset as u64,
        "jpeg_thumbnail",
        Exactness::Derived,
    );
}

#[test]
fn compressed_toc_section_propagates_expansion_limit() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    let jpeg = jpeg_payload();
    let compressed = unix_compress_literals(&jpeg);
    let data = build_toc_section_prt("THMB_IMG_MAIN", &compressed, jpeg.len());
    let mut options = DecodeOptions {
        container_only: true,
        policy: DecodePolicy::service(),
    };
    options.policy.limits.max_decompressed_bytes_per_expand =
        u64::try_from(jpeg.len() - 1).expect("fixture length fits the resource limit");
    let error = CreoCodec
        .decode(&mut Cursor::new(data.clone()), &options)
        .expect_err("TOC section expansion must propagate the resource refusal");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::DecompressedBytes
                && limit.operation == "begin_expand"
    ));

    options.policy.limits.max_decompressed_bytes_per_expand =
        u64::try_from(jpeg.len()).expect("fixture length fits the resource limit");
    CreoCodec
        .decode(&mut Cursor::new(data), &options)
        .expect("the exact per-expansion limit admits the section");
}

#[test]
fn decode_propagates_spline_grid_collection_limit() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    let mut payload =
        b"srf_array\0\xf8\0srf_prim_ptr(spline)\0\xe0\x02i_points\0\xf9\x02\x03".to_vec();
    payload.extend([0x0f; 6]);
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let mut options = DecodeOptions {
        policy: DecodePolicy::service(),
        ..DecodeOptions::default()
    };
    options.policy.limits.max_collection_items = 5;
    let error = CreoCodec
        .decode(&mut Cursor::new(data.clone()), &options)
        .expect_err("six scalar slots exceed the five-item limit");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "admit Creo spline scalar grid"
    ));

    options.policy.limits.max_collection_items = 6;
    CreoCodec
        .decode(&mut Cursor::new(data), &options)
        .expect("the exact grid item limit admits the fixture");
}

#[test]
fn decode_propagates_counted_scalar_array_collection_limit() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    let payload =
        b"srf_array\0\xf8\0srf_prim_ptr(splsrf)\0\xe0\x02u_params\0\xf8\x04\x0f\x0f\x0f\x0f\xe3";
    let data = build_prt("c", &[("VisibGeom", payload.to_vec())]);
    let mut options = DecodeOptions {
        policy: DecodePolicy::service(),
        ..DecodeOptions::default()
    };
    options.policy.limits.max_collection_items = 3;
    let error = CreoCodec
        .decode(&mut Cursor::new(data.clone()), &options)
        .expect_err("four scalar slots exceed the three-item limit");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "admit Creo counted scalar array"
    ));

    options.policy.limits.max_collection_items = 4;
    CreoCodec
        .decode(&mut Cursor::new(data), &options)
        .expect("the exact counted scalar item limit admits the fixture");
}

#[test]
fn decode_projects_orphan_geometry_generator_as_stored_geometry() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let data = build_prt("c", &[("VisibGeom", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("geometry generator feature");

    assert!(matches!(
        feature.evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::StoredGeometry {}
        )
    ));
    assert_eq!(
        wire::coverage_count(
            result.report(),
            crate::coverage::TRANSFERRED_GEOMETRY_GENERATOR_FEATURE_COUNT.as_str()
        ),
        1
    );
    assert_eq!(
        wire::coverage_count(
            result.report(),
            crate::coverage::TRANSFERRED_NATIVE_FEATURE_COUNT.as_str()
        ),
        0
    );
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#7")
        .expect("retained unresolved surface carrier");
    assert!(matches!(
        surface.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
            record: Some(_)
        })
    ));
    assert_eq!(
        wire::coverage_count(
            result.report(),
            crate::coverage::RETAINED_UNKNOWN_VISIBLE_SURFACE_ROW_COUNT.as_str()
        ),
        1
    );
    assert_eq!(
        wire::coverage_count(
            result.report(),
            crate::coverage::UNTRANSFERRED_VISIBLE_SURFACE_ROW_COUNT.as_str()
        ),
        1
    );
}

#[test]
fn decode_binds_ordered_visible_surfaces_to_matching_replay_runs() {
    let mut visible = b"srf_array\0\xf8\x02".to_vec();
    visible.extend_from_slice(&[7, 0x24, 4, 0x01, 0, 8, 0xe3]);
    visible.extend_from_slice(&[8, 0x26, 4, 0x01, 0, 0, 0xe3]);
    visible.extend_from_slice(b"crv_array\0\xf3\xf8\0");
    let mut nonvisible = b"srf_array\0\xf8\x05".to_vec();
    for (id, kind) in [(9, 0x24), (10, 0x26), (11, 0x22), (12, 0x24), (13, 0x26)] {
        nonvisible.extend_from_slice(&[id, kind, 4, 0x01, 0, 0, 0xe3]);
    }
    nonvisible.extend_from_slice(b"crv_array\0\xf3\xf8\0");
    let mut allfeatur = allfeatur_row(4, [0xeb, 0x04], 913, &[0xf8, 7, 0xf7, 79, 0xfb, 0xe3]);
    for (id, class_id) in [
        (7, 254),
        (8, 254),
        (9, 214),
        (10, 214),
        (11, 215),
        (12, 214),
        (13, 214),
    ] {
        allfeatur.extend_from_slice(&[id, 0x80, class_id, 0, 0, 0xe3]);
    }
    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt(
                "c",
                &[
                    ("VisibGeom", visible),
                    ("NovisGeom", nonvisible),
                    ("AllFeatur", allfeatur),
                    ("MdlStatus", b"Round id 4\0".to_vec()),
                ],
            )),
            &DecodeOptions::default(),
        )
        .expect("decode ordered surface replay");

    let associations =
        &result.ir().native.namespace("creo").unwrap().arenas()["feature_surface_replays"];
    assert_eq!(associations.len(), 4);
    for (association, visible_id, replay_id, ordinal) in [
        (&associations[0], 7, 9, 0),
        (&associations[1], 8, 10, 0),
        (&associations[2], 7, 12, 1),
        (&associations[3], 8, 13, 1),
    ] {
        assert_eq!(association.fields()["owner_feature_id"], 4);
        assert_eq!(association.fields()["visible_surface_id"], visible_id);
        assert_eq!(association.fields()["replay_surface_id"], replay_id);
        assert_eq!(association.fields()["replay_ordinal"], ordinal);
    }
    assert_eq!(
        wire::coverage_count(
            result.report(),
            crate::coverage::DECODED_FEATURE_SURFACE_REPLAY_ASSOCIATION_COUNT.as_str()
        ),
        4
    );
}

#[test]
fn decode_annotations_cover_every_emitted_entity() {
    let mut datum = b"srf_array\0\xf8\x01".to_vec();
    datum.extend([4, 0x22, 1, 1, 1, 0]);
    datum.extend([0x0f; 4]);
    for value in [2.0_f64, 0.0, 3.0, -2.0, 0.0, -3.0] {
        if value == 0.0 {
            datum.push(0x0f);
        } else {
            let mut bytes = value.to_be_bytes();
            bytes[0] = if value.is_sign_negative() { 0x2d } else { 0x46 };
            datum.extend(bytes);
        }
    }
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", visibgeom_payload(1, 0)),
            ("NovisGeom", vec![0xaa, 0xbb]),
            ("ActDatums", datum),
        ],
    );
    let datum_offset =
        container::scan_bytes_ok(data.clone()).planes.datums[0].offset_in_payload as u64;
    let mut reader = Cursor::new(data);
    let result = EditableDecodeResult::from(
        CreoCodec
            .decode(&mut reader, &DecodeOptions::default())
            .expect("decode"),
    );

    let unknowns = result.ir().native_unknowns("creo").unwrap();
    assert_eq!(unknowns.len(), 3);
    assert_eq!(result.ir().model.surfaces.len(), 1);
    for unknown in &unknowns {
        let section_name = unknown
            .id
            .as_str()
            .strip_prefix("creo:")
            .and_then(|suffix| suffix.split_once(":section#"))
            .map(|(name, _)| name)
            .expect("unknown id contains its source section");
        let retained = result
            .source_fidelity()
            .retained_record(unknown.id.as_str())
            .expect("unknown source record");
        assert_annotation(
            &result.source_fidelity().annotations,
            unknown.id.as_str(),
            &format!("creo:{section_name}"),
            retained.offset(),
            "psb_geometry_section",
            Exactness::Unknown,
        );
    }
    for surface in &result.ir().model.surfaces {
        assert_annotation(
            &result.source_fidelity().annotations,
            surface.id.as_str(),
            "creo:ActDatums",
            datum_offset,
            "datum_plane_outline",
            Exactness::Derived,
        );
    }
    let emitted_entity_count =
        unknowns.len() + result.ir().model.surfaces.len() + result.ir().model.features.len();
    assert_eq!(
        result.source_fidelity().annotations.provenance.len(),
        emitted_entity_count
    );
    assert_eq!(
        result.source_fidelity().annotations.exactness().len(),
        emitted_entity_count
    );
}

#[test]
fn decode_retains_mdlstatus_states_and_projects_only_agreement() {
    let data = build_prt(
        "c",
        &[(
            "MdlStatus",
            b"noise\0xProtrusion id 40\0Round id 41\0Future Feature id 42\0Datum Plane id 43\0Draft id 44\0Hole id 40\0ySurface id 45\0"
                .to_vec(),
        )],
    );
    let scan = container::scan_bytes_ok(data.clone());
    assert_eq!(scan.features.operation_states.len(), 7);
    assert_eq!(scan.features.operation_states[0].feature_id, 40);
    assert_eq!(
        scan.features.operation_states[0].kind.as_str(),
        "Protrusion"
    );
    assert_eq!(
        scan.features.operation_states[0].stored_name().as_deref(),
        Some("xProtrusion id 40")
    );
    assert_eq!(
        scan.features.operation_states[0].name.identifier_keyword(),
        Some("id")
    );
    assert_eq!(
        scan.features.operation_states[0].state_offset + 1,
        scan.features.operation_states[0].offset
    );
    assert_eq!(scan.features.operation_states[5].feature_id, 40);
    assert_eq!(scan.features.operation_states[5].kind.as_str(), "Hole");
    assert!(scan.features.operation_states[0].display_state_conflict);
    assert!(scan.features.operation_states[5].display_state_conflict);
    assert_eq!(scan.features.operations.len(), 6);
    assert_eq!(scan.features.operations[0].feature_id, 40);
    assert_eq!(scan.features.operations[0].kind.as_str(), "Native Feature");
    assert!(!scan.features.operations[0].display_name_stored());
    assert!(scan.features.operations[0].display_state_conflict);
    assert_eq!(scan.features.operations[0].stored_name_prefix(), None);
    assert_eq!(scan.features.operations[1].feature_id, 41);
    assert_eq!(scan.features.operations[1].kind.as_str(), "Round");
    assert_eq!(scan.features.operations[2].kind.as_str(), "Future Feature");
    assert_eq!(scan.features.operations[3].kind.as_str(), "Datum Plane");
    assert_eq!(scan.features.operations[4].kind.as_str(), "Draft");
    assert_eq!(scan.features.operations[5].kind.as_str(), "Surface");
    assert_eq!(scan.features.operations[5].stored_name_prefix(), Some(b'y'));

    let result = EditableDecodeResult::from(
        CreoCodec
            .decode(&mut Cursor::new(data), &DecodeOptions::default())
            .expect("decode"),
    );
    let states =
        &result.ir().native.namespace("creo").unwrap().arenas()["feature_operation_states"];
    assert_eq!(states.len(), 7);
    let feature_40 = states
        .iter()
        .filter(|state| state.fields()["feature_id"] == 40)
        .collect::<Vec<_>>();
    assert_eq!(feature_40.len(), 2);
    assert_eq!(feature_40[0].fields()["state_ordinal"], 0);
    assert_eq!(feature_40[0].fields()["current"], false);
    assert_eq!(feature_40[0].fields()["stored_name"], "xProtrusion id 40");
    assert_eq!(
        feature_40[0].fields()["stored_name_bytes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|byte| byte.as_u64().unwrap() as u8)
            .collect::<Vec<_>>(),
        b"xProtrusion id 40"
    );
    assert_eq!(feature_40[0].fields()["identifier_keyword"], "id");
    assert_eq!(feature_40[0].fields()["display_state_conflict"], true);
    assert_eq!(feature_40[1].fields()["state_ordinal"], 1);
    assert_eq!(feature_40[1].fields()["current"], false);
    assert_eq!(feature_40[1].fields()["display_state_conflict"], true);
    assert_eq!(result.ir().model.features.len(), 6);
    assert_eq!(
        result.ir().model.features[0].id.as_str(),
        "creo:model:feature#40"
    );
    assert_eq!(result.ir().model.features[0].ordinal, 0);
    assert_eq!(
        result.ir().model.features[1].id.as_str(),
        "creo:model:feature#41"
    );
    assert_eq!(result.ir().model.features[1].ordinal, 1);
    assert!(matches!(
        result.ir().model.features[0].evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Native { kind, .. })
            if kind.as_str() == "Native Feature"
    ));
    assert_annotation(
        &result.source_fidelity().annotations,
        "creo:model:feature#40",
        "creo:MdlStatus",
        scan.features.operations[0].offset as u64,
        "feature_operation_state_consensus",
        Exactness::Derived,
    );
    assert!(matches!(
        result.ir().model.features[1].evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Fillet {
            groups,
        }) if matches!(groups.as_slice(), [group]
            if matches!(group.edges, cadmpeg_ir::features::EdgeSelection::Unresolved)
                && group.radius.is_unresolved())
    ));
    assert_eq!(
        result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.id.as_str() == "creo:model:feature#45")
            .expect("state-prefixed feature")
            .source_properties
            .get("mdl_stored_name_prefix")
            .map(String::as_str),
        Some("y")
    );
    assert_annotation(
        &result.source_fidelity().annotations,
        "creo:model:feature#41",
        "creo:MdlStatus",
        scan.features.operations[1].offset as u64,
        "feature_operation_name",
        Exactness::ByteExact,
    );
}

#[test]
fn decode_preserves_stored_feature_identifier_keyword() {
    let data = build_prt("c", &[("MdlStatus", b"ySurface ID 45\0".to_vec())]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#45")
        .expect("surface feature");

    assert_eq!(feature.name.as_deref(), Some("Surface ID 45"));
    assert_eq!(
        feature
            .source_properties
            .get("mdl_stored_name_prefix")
            .map(String::as_str),
        Some("y")
    );
}

#[test]
fn decode_reports_only_unimplemented_relation_function_namespaces() {
    let data = build_prt("c", &[]);
    let decoded = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode empty part");

    let relation_gap = decoded
        .report()
        .losses
        .iter()
        .find(|loss| loss.message.contains("cross-model relation functions"))
        .expect("precise remaining relation gap");
    assert!(relation_gap.message.contains("graph, case-study, cabling"));
    assert!(!relation_gap.message.contains("pattern-matching"));
}

#[test]
fn decode_is_honest_geometryless_with_preserved_sections() {
    let mut visible = visibgeom_payload(5, 12);
    visible.extend_from_slice(b"_principal_sys_units_id\0\x33");
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", visible),
            ("NovisGeom", vec![0xaa, 0xbb]),
            ("AllFeatur", vec![0x01]),
        ],
    );
    let mut reader = Cursor::new(data);
    let result = CreoCodec
        .decode(&mut reader, &DecodeOptions::default())
        .expect("decode");

    assert!(!result.report().geometry_transferred());
    // The two PSB geometry sections are preserved as unknown records.
    let unknowns = result.ir().native_unknowns("creo").unwrap();
    assert_eq!(unknowns.len(), 2);
    assert!(unknowns.iter().any(|u| u.id.as_str().contains("VisibGeom")));
    assert!(unknowns.iter().any(|u| u.id.as_str().contains("NovisGeom")));
    // No geometry arenas populated.
    assert!(result.ir().model.surfaces.is_empty());
    assert!(result.ir().model.points.is_empty());
    assert!(result.ir().model.faces.is_empty());
    // Source attributes carry the census.
    let source = result.ir().source.as_ref().expect("source");
    assert_eq!(
        source.attributes.get("srf_array_count").map(String::as_str),
        Some("5")
    );
    assert_eq!(
        source.attributes.get("crv_array_count").map(String::as_str),
        Some("12")
    );
    assert_eq!(
        source.attributes.get("principal_unit").map(String::as_str),
        Some("mmNs")
    );
    // A blocking loss note names the prototype-vs-instance limitation.
    assert!(result
        .report()
        .losses
        .iter()
        .any(|l| l.message.contains("prototype")));
}

#[test]
fn decode_admits_binary_inch_principal_unit() {
    let mut visible = visibgeom_payload(0, 0);
    visible.extend_from_slice(b"_principal_sys_units_id\0\x36");
    let data = build_prt("c", &[("VisibGeom", visible)]);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode binary inch unit selector");

    let source = result.ir().source.as_ref().expect("source metadata");
    assert_eq!(
        source.attributes.get("principal_unit").map(String::as_str),
        Some("inLbmS")
    );
    assert_eq!(
        source
            .attributes
            .get("source_length_scale_mm")
            .map(String::as_str),
        Some("25.4")
    );
}

#[test]
fn container_only_preserves_sections_without_transferring_entities() {
    let mut geometry = visibgeom_payload(1, 0);
    geometry.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    let data = build_prt(
        "c",
        &[
            ("VisibGeom", geometry),
            ("MdlStatus", b"Datum Plane id 4\0".to_vec()),
        ],
    );
    let result = CreoCodec
        .decode(
            &mut Cursor::new(data),
            &DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        )
        .expect("container decode");

    assert!(result.report().container_only());
    assert!(!result.report().geometry_transferred());
    assert!(result.ir().model.surfaces.is_empty());
    assert!(result.ir().model.features.is_empty());
    assert_eq!(result.ir().native_unknowns("creo").unwrap().len(), 1);
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.starts_with("Transferred ")));
}
