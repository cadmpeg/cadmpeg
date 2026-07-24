// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::{Codec, CodecEntry, Confidence, DecodeOptions};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Cursor;

use crate::IgesCodec;

fn card(data: &[u8], section: u8, sequence: u32) -> Vec<u8> {
    card_with_ending(data, section, sequence, b"\n")
}

fn card_with_ending(data: &[u8], section: u8, sequence: u32, ending: &[u8]) -> Vec<u8> {
    assert!(data.len() <= 72);
    let mut card = vec![b' '; 80];
    card[..data.len()].copy_from_slice(data);
    card[72] = section;
    card[73..80].copy_from_slice(format!("{sequence:>7}").as_bytes());
    card.extend_from_slice(ending);
    card
}

#[test]
fn fixed_ascii_detection_requires_two_consistent_cards() {
    let mut valid = card(b"generated fixture", b'S', 1);
    valid.extend(card(b"", b'G', 1));
    assert_eq!(IgesCodec.detect(&valid), Confidence::High);

    assert_eq!(IgesCodec.detect(&valid[..81]), Confidence::No);

    let mut arbitrary = vec![b'x'; 72];
    arbitrary.extend_from_slice(b"S      1\nsecond line\n");
    assert_eq!(IgesCodec.detect(&arbitrary), Confidence::No);
}

#[test]
fn malformed_sequence_padding_is_rejected_without_panicking() {
    let mut bytes = point_file();
    bytes[73..80].copy_from_slice(b"     1 ");

    assert_eq!(IgesCodec.detect(&bytes), Confidence::No);
    assert_eq!(
        IgesCodec
            .inspect(
                &mut Cursor::new(bytes),
                &cadmpeg_ir::decode::InspectOptions::default()
            )
            .unwrap_err()
            .to_string(),
        "not the expected format: unrecognized IGES representation"
    );
}

#[test]
fn single_target_cycle_detection_handles_long_file_controlled_chains_iteratively() {
    let targets = (1..=100_000_u32)
        .map(|sequence| (sequence, sequence + 1))
        .collect::<BTreeMap<_, _>>();
    let mut visited = std::collections::BTreeSet::new();

    assert!(!crate::entities::structure::single_target_cycle(
        1,
        &targets,
        &mut visited
    ));
    assert_eq!(visited.len(), 100_000);

    let mut cyclic = targets;
    cyclic.insert(100_001, 50_000);
    assert!(crate::entities::structure::single_target_cycle(
        1,
        &cyclic,
        &mut std::collections::BTreeSet::new()
    ));
}

#[test]
fn directed_cycle_detection_handles_long_branching_graphs_iteratively() {
    let mut graph = (1..=100_000_u32)
        .map(|sequence| (sequence, vec![sequence + 1]))
        .collect::<BTreeMap<_, _>>();
    graph.entry(50_000).or_default().push(100_001);
    let mut visited = std::collections::BTreeSet::new();

    assert!(!crate::entities::directed_cycle(
        1,
        &mut visited,
        |sequence| graph.get(&sequence).cloned().unwrap_or_default()
    ));
    assert_eq!(visited.len(), 100_001);

    graph.insert(100_001, vec![50_000]);
    assert!(crate::entities::directed_cycle(
        1,
        &mut std::collections::BTreeSet::new(),
        |sequence| graph.get(&sequence).cloned().unwrap_or_default()
    ));
}

#[test]
fn envelope_admission_exactly_matches_the_machine_matrix() {
    let matrix_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/iges-envelope-a.toml");
    let source = std::fs::read_to_string(matrix_path).unwrap();
    let matrix = toml::from_str::<toml::Value>(&source).unwrap();
    let mut admitted = BTreeMap::<i64, Option<Vec<i64>>>::new();
    for entity in matrix["entity"].as_array().unwrap() {
        let entity_type = entity["type"].as_integer().unwrap();
        let forms = if entity["forms"].as_str() == Some("implementor-defined") {
            None
        } else {
            Some(
                entity["forms"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|form| form.as_integer().unwrap())
                    .collect(),
            )
        };
        assert!(admitted.insert(entity_type, forms).is_none());
        for required in ["name", "domain", "decoder", "destination"] {
            assert!(entity[required]
                .as_str()
                .is_some_and(|value| !value.is_empty()));
        }
        for required in ["fixture_classes", "assertions"] {
            assert!(entity[required]
                .as_array()
                .is_some_and(|values| !values.is_empty()));
        }
    }
    for entity_type in 0..=600 {
        for form in -1..=100 {
            let expected = admitted.get(&entity_type).is_some_and(|forms| {
                forms
                    .as_ref()
                    .map_or(matches!(form, 5001..=9999), |forms| forms.contains(&form))
            });
            assert_eq!(
                crate::profile::envelope_a_admits(entity_type, form),
                expected,
                "entity type {entity_type} form {form}"
            );
        }
    }
    for form in [101, 5000, 5001, 9999, 10000] {
        assert_eq!(
            crate::profile::envelope_a_admits(302, form),
            matches!(form, 5001..=9999)
        );
    }
}

#[test]
fn every_admitted_entity_form_routes_to_a_typed_decoder() {
    let matrix_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/iges-envelope-a.toml");
    let source = std::fs::read_to_string(matrix_path).unwrap();
    let matrix = toml::from_str::<toml::Value>(&source).unwrap();
    let entities = matrix["entity"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|entity| {
            let entity_type = entity["type"].as_integer().unwrap();
            let forms = entity["forms"].as_array().map_or_else(
                || vec![5001, 9999],
                |forms| {
                    forms
                        .iter()
                        .map(|form| form.as_integer().unwrap())
                        .collect()
                },
            );
            forms.into_iter().map(move |form| OwnedTestEntity {
                entity_type,
                form,
                label: format!("E{entity_type}"),
                status: "00000000",
                parameters: format!("{entity_type};"),
            })
        })
        .collect::<Vec<_>>();
    let bytes = owned_test_file(&entities);

    let result = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let generic_fallthroughs = result
        .report
        .losses
        .iter()
        .filter(|loss| {
            loss.message
                .ends_with("retained without neutral projection")
        })
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    assert!(generic_fallthroughs.is_empty(), "{generic_fallthroughs:#?}");
}

#[test]
fn repeated_decode_is_canonical() {
    let bytes = explicit_tetrahedron_solid_with_boolean_file();
    let first = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let second = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(
        first.ir.to_canonical_json().unwrap(),
        second.ir.to_canonical_json().unwrap()
    );
    assert_eq!(
        serde_json::to_vec(&first.report).unwrap(),
        serde_json::to_vec(&second.report).unwrap()
    );
    assert_eq!(first.source_fidelity, second.source_fidelity);
}

#[test]
fn cumulative_l8_domain_fixtures_validate_without_loss() {
    let (void_solid, _, _, _) = explicit_void_solid_file();
    let fixtures = [
        ("point", point_file()),
        (
            "conic",
            conic_arc_file(0, b"104,0.25,0,1,0,0,-1,0,2,0,0,1;"),
        ),
        ("nurbs-curve", rational_nurbs_curve_file()),
        ("spline-surface", parametric_spline_surface_file()),
        ("revolution", surface_of_revolution_file()),
        ("trimmed-sheet", trimmed_plane_with_inner_loop_file()),
        (
            "manifold-solid",
            explicit_tetrahedron_solid_with_boolean_file(),
        ),
        ("void-solid", void_solid),
        (
            "non-manifold-shell",
            explicit_non_manifold_open_shell_file(),
        ),
        ("appearance", colored_explicit_vertex_loop_file()),
        ("csg", primitive_solids_file()),
        ("solid-assembly", solid_assembly_file()),
        ("subfigures", nested_subfigure_file()),
        ("network", connected_network_subfigure_file()),
        ("external-references", external_reference_forms_file()),
        ("attribute-definitions", attribute_definition_forms_file()),
        ("attribute-instances", attribute_instance_forms_file()),
        ("properties", variable_schema_property_forms_file()),
        ("views", view_visibility_forms_file()),
        ("drawing", drawing_with_properties_file()),
        ("text", text_annotation_file()),
        ("dimensions", dimension_forms_file()),
        ("symbols", symbol_and_sectioned_area_file()),
        ("associativity", bounded_associativity_forms_file()),
        ("text-font", text_font_definition_file()),
        ("units-data", units_data_file()),
    ];

    for (name, bytes) in fixtures {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(bytes.as_slice()),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert!(
            result.report.losses.is_empty(),
            "{name}: {:#?}",
            result.report.losses
        );
        let validation = cadmpeg_ir::validate::validate_with_source_fidelity(
            &result.ir,
            &result.source_fidelity,
            Vec::new(),
        );
        assert!(validation.is_ok(), "{name}: {:#?}", validation.findings);
    }
}

#[test]
fn decode_names_forms_outside_the_closed_envelope() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 430,
        form: 2,
        label: "BADFORM".into(),
        status: "00000000",
        parameters: "430,0;".into(),
    }]);
    let result = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result.report.losses.iter().any(|loss| {
        loss.message
            == "IGES entity type 430 form 2 is outside the Fixed ASCII mechanical/document envelope"
    }));
}

#[test]
fn inspect_reports_sections_and_physical_line_endings() {
    let mut bytes = card_with_ending(b"original fixture", b'S', 1, b"\r\n");
    bytes.extend(card_with_ending(b"1H,,1H;,,;", b'G', 1, b"\n"));
    bytes.extend(card_with_ending(
        b"S0000001G0000001D0000000P0000000",
        b'T',
        1,
        b"\r",
    ));

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_ir::decode::InspectOptions::default(),
        )
        .unwrap();

    assert_eq!(summary.format, "iges");
    assert_eq!(summary.container_kind, "fixed-ascii");
    assert_eq!(summary.entries.len(), 3);
    assert_eq!(summary.entries[0].name, "start");
    assert_eq!(summary.entries[0].attributes["line_endings"], "crlf:1");
    assert_eq!(summary.entries[1].attributes["line_endings"], "lf:1");
    assert_eq!(summary.entries[2].attributes["line_endings"], "cr:1");
}

#[test]
fn decode_retains_short_and_extended_physical_records_before_terminate() {
    let mut bytes = point_file();
    let mut inserted = b"short record\n".to_vec();
    inserted.extend(std::iter::repeat_n(b'x', 81));
    inserted.push(b'\n');
    bytes.splice(162..162, inserted);

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes.as_slice()),
            &cadmpeg_ir::decode::InspectOptions::default(),
        )
        .unwrap();
    let noncanonical = summary
        .entries
        .iter()
        .find(|entry| entry.name == "noncanonical-physical-records")
        .unwrap();
    assert_eq!(noncanonical.role, "retained-opaque-records");
    assert_eq!(noncanonical.attributes["records"], "2");
}

fn fixed_ascii_with_global(global: &[u8]) -> Vec<u8> {
    let mut bytes = card(b"original fixture", b'S', 1);
    let chunks = global.chunks(72).collect::<Vec<_>>();
    for (index, chunk) in chunks.iter().enumerate() {
        bytes.extend(card(chunk, b'G', u32::try_from(index + 1).unwrap()));
    }
    bytes.extend(card(
        format!("S0000001G{:07}D0000000P0000000", chunks.len()).as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn directory_card(fields: [&str; 9], sequence: u32) -> Vec<u8> {
    let data = fields.into_iter().fold(String::new(), |mut data, field| {
        write!(data, "{field:>8}").unwrap();
        data
    });
    card(data.as_bytes(), b'D', sequence)
}

fn parameter_card(data: &[u8], directory_sequence: u32, sequence: u32) -> Vec<u8> {
    assert!(data.len() <= 64);
    let mut payload = vec![b' '; 72];
    payload[..data.len()].copy_from_slice(data);
    payload[64..72].copy_from_slice(format!("{directory_sequence:>8}").as_bytes());
    card(&payload, b'P', sequence)
}

fn parameter_cards(data: &[u8], directory_sequence: u32, first_sequence: u32) -> Vec<u8> {
    data.chunks(64)
        .enumerate()
        .flat_map(|(index, chunk)| {
            parameter_card(
                chunk,
                directory_sequence,
                first_sequence + u32::try_from(index).unwrap(),
            )
        })
        .collect()
}

#[test]
fn inspect_parses_alternate_delimiters_and_cross_card_hollerith() {
    let product = "p".repeat(70);
    let global = format!(
        "1H^^1H!^70H{product}^8Hpart.igs^7Hcadmpeg^3H0.1^32^38^6^308^15^0H^1.0^2^2HMM^1^1.0^15H20260714.000000^0.001^1000.0^6Hauthor^3Horg^11^0^0H^0H!"
    );
    let bytes = fixed_ascii_with_global(global.as_bytes());

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_ir::decode::InspectOptions::default(),
        )
        .unwrap();

    assert!(summary.notes.contains(&"parameter_delimiter=^".into()));
    assert!(summary.notes.contains(&"record_delimiter=!".into()));
    assert!(summary.notes.contains(&format!("sender_product={product}")));
    assert!(summary.notes.contains(&"iges_version=5.3".into()));
    assert!(summary.notes.contains(&"units=MM".into()));
}

#[test]
fn inspect_reports_directory_entity_and_form_census() {
    let bytes = point_file();

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_ir::decode::InspectOptions::default(),
        )
        .unwrap();

    assert!(summary.notes.contains(&"entities=1".into()));
    assert!(summary.notes.contains(&"entity.116.form.0=1".into()));
    assert!(summary.notes.contains(&"parameter_records=1".into()));
    assert!(summary.notes.contains(&"parameter_tokens=4".into()));
}

fn point_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["116", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        ["116", "0", "0", "1", "0", "", "", "POINT", "0"],
        2,
    ));
    bytes.extend(parameter_card(b"116,1.0,2.0,3.0;", 1, 1));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn direction_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["123", "1", "0", "0", "0", "0", "0", "0", "00010000"],
        1,
    ));
    bytes.extend(directory_card(
        ["123", "0", "0", "1", "0", "", "", "VECTOR", "0"],
        2,
    ));
    bytes.extend(parameter_card(b"123,2,-3,4;", 1, 1));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

#[test]
fn decode_retains_a_typed_dimensionless_direction() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(direction_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(!result.report.geometry_transferred);
    assert!(result.report.losses.is_empty());
    let native = result.ir.native.namespace("iges").unwrap();
    assert_eq!(native.arenas["directions"].len(), 1);
    let components = native.arenas["directions"][0].fields["components"]
        .as_array()
        .unwrap();
    assert_eq!(components[0], 2.0);
    assert_eq!(components[1], -3.0);
    assert_eq!(components[2], 4.0);
    assert_eq!(
        native.arenas["directions"][0].fields["physically_dependent"],
        true
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

fn line_file(form: i64) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        [
            "110",
            "1",
            "0",
            "0",
            "4",
            "0",
            "0",
            "0",
            if form == 0 { "00000000" } else { "00000600" },
        ],
        1,
    ));
    bytes.extend(directory_card(
        ["110", "0", "0", "1", &form.to_string(), "", "", "LINE", "0"],
        2,
    ));
    bytes.extend(parameter_card(b"110,1,2,3,4,6,3;", 1, 1));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn circular_arc_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["100", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        ["100", "0", "0", "1", "0", "", "", "ARC", "0"],
        2,
    ));
    bytes.extend(parameter_card(b"100,0,0,0,1,0,0,1;", 1, 1));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn uniform_offset_circle_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["100", "1", "0", "0", "0", "0", "0", "0", "00010000"],
        1,
    ));
    bytes.extend(directory_card(
        ["100", "0", "0", "1", "0", "", "", "ARC", "0"],
        2,
    ));
    bytes.extend(directory_card(
        ["130", "2", "0", "0", "0", "0", "0", "0", "00000000"],
        3,
    ));
    bytes.extend(directory_card(
        ["130", "0", "0", "1", "0", "", "", "OFFSET", "0"],
        4,
    ));
    bytes.extend(parameter_card(b"100,0,0,0,2,0,0,2;", 1, 1));
    bytes.extend(parameter_card(
        b"130,1,1,0,0,0,0.5,0,0,0,0,0,1,0,1.5707963267948966;",
        3,
        2,
    ));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000004P0000002").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn linear_offset_line_file(basis: i64) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["110", "1", "0", "0", "0", "0", "0", "0", "00010000"],
        1,
    ));
    bytes.extend(directory_card(
        ["110", "0", "0", "1", "0", "", "", "LINE", "0"],
        2,
    ));
    bytes.extend(directory_card(
        ["130", "2", "0", "0", "0", "0", "0", "0", "00000000"],
        3,
    ));
    bytes.extend(directory_card(
        ["130", "0", "0", "1", "0", "", "", "OFFSET", "0"],
        4,
    ));
    bytes.extend(parameter_card(b"110,0,0,0,10,0,0;", 1, 1));
    let control_end = if basis == 1 { 10 } else { 1 };
    bytes.extend(parameter_card(
        format!("130,1,2,0,0,{basis},1,0,3,{control_end},0,0,1,0,1;").as_bytes(),
        3,
        2,
    ));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000004P0000002").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn function_offset_line_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, parameter_start, entity_type, label, status) in [
        (1, 1, 110, "LINE", "00010000"),
        (3, 2, 126, "LAW", "00010000"),
        (5, 3, 130, "OFFSET", "00000000"),
    ] {
        let entity_type = entity_type.to_string();
        let parameter_start = parameter_start.to_string();
        bytes.extend(directory_card(
            [
                &entity_type,
                &parameter_start,
                "0",
                "0",
                "0",
                "0",
                "0",
                "0",
                status,
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [&entity_type, "0", "0", "1", "0", "", "", label, "0"],
            sequence + 1,
        ));
    }
    bytes.extend(parameter_card(b"110,0,0,0,10,0,0;", 1, 1));
    bytes.extend(parameter_card(
        b"126,1,1,1,0,1,0,0,0,1,1,1,1,0,1,0,1,3,0,0,1,0,0,1;",
        3,
        2,
    ));
    bytes.extend(parameter_card(b"130,1,3,3,2,2,0,0,0,0,0,0,1,0,1;", 5, 3));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000006P0000003").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn composite_curve_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, parameter_start, entity_type, label, status) in [
        (1, 1, "110", "CHILD1", "00010000"),
        (3, 2, "110", "CHILD2", "00010000"),
        (5, 3, "102", "COMPOSIT", "00000000"),
    ] {
        bytes.extend(directory_card(
            [
                entity_type,
                &parameter_start.to_string(),
                "0",
                "0",
                "0",
                "0",
                "0",
                "0",
                status,
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [entity_type, "0", "0", "1", "0", "", "", label, "0"],
            sequence + 1,
        ));
    }
    bytes.extend(parameter_card(b"110,0,0,0,1,0,0;", 1, 1));
    bytes.extend(parameter_card(b"110,1,0,0,1,1,0;", 3, 2));
    bytes.extend(parameter_card(b"102,2,1,3;", 5, 3));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000006P0000003").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn mixed_analytic_composite_curve_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, parameter_start, entity_type, label, status) in [
        (1, 1, "100", "ARC", "00010000"),
        (3, 2, "110", "LINE", "00010000"),
        (5, 3, "102", "COMPOSIT", "00000000"),
    ] {
        bytes.extend(directory_card(
            [
                entity_type,
                &parameter_start.to_string(),
                "0",
                "0",
                "0",
                "0",
                "0",
                "0",
                status,
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [entity_type, "0", "0", "1", "0", "", "", label, "0"],
            sequence + 1,
        ));
    }
    bytes.extend(parameter_card(b"100,0,0,0,1,0,0,1;", 1, 1));
    bytes.extend(parameter_card(b"110,0,1,0,0,2,0;", 3, 2));
    bytes.extend(parameter_card(b"102,2,1,3;", 5, 3));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000006P0000003").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

#[test]
fn decode_concatenates_ordered_composite_curve_children() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(composite_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir.model.procedural_curves.len(), 1);
    let composite = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D5")
        .unwrap();
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &composite.geometry else {
        panic!("expected a concatenated NURBS cache");
    };
    assert_eq!(nurbs.knots, vec![0.0, 0.0, 1.0, 2.0, 2.0]);
    assert_eq!(nurbs.control_points.len(), 3);
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point(1, &nurbs.knots, &nurbs.control_points, None, 1.5),
        Some(cadmpeg_ir::math::Point3::new(1.0, 0.5, 0.0))
    );
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_concatenates_exact_circular_arc_and_line_children() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(mixed_analytic_composite_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let composite = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D5")
        .unwrap();
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &composite.geometry else {
        panic!("expected an exact quadratic composite cache");
    };
    assert_eq!(nurbs.degree, 2);
    assert_eq!(nurbs.control_points.len(), 5);
    assert_eq!(
        nurbs.weights.as_ref().unwrap()[1],
        std::f64::consts::FRAC_1_SQRT_2
    );
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

fn copious_data_file(form: i64, parameters: &[u8], status: &str) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let parameter_count = parameters.len().div_ceil(64);
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["106", "1", "0", "0", "0", "0", "0", "0", status],
        1,
    ));
    bytes.extend(directory_card(
        [
            "106",
            "0",
            "0",
            &parameter_count.to_string(),
            &form.to_string(),
            "",
            "",
            "COPIOUS",
            "0",
        ],
        2,
    ));
    bytes.extend(parameter_cards(parameters, 1, 1));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P{parameter_count:07}").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

#[test]
fn decode_projects_copious_linear_paths_with_segment_parameters() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(copious_data_file(
                12,
                b"106,2,3,0,0,0,1,0,0,1,2,0;",
                "00000000",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(path) = &result.ir.model.curves[0].geometry
    else {
        panic!("expected a degree-one path carrier");
    };
    assert_eq!(path.degree, 1);
    assert_eq!(path.knots, vec![0.0, 0.0, 1.0, 2.0, 2.0]);
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point(1, &path.knots, &path.control_points, None, 1.5),
        Some(cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0))
    );
    assert_eq!(result.ir.model.edges[0].param_range, Some([0.0, 2.0]));
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_separates_copious_points_vectors_and_presentation_forms() {
    let points = IgesCodec
        .decode(
            &mut Cursor::new(copious_data_file(
                3,
                b"106,3,2,1,2,3,0,0,1,4,5,6,1,0,0;",
                "00000000",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(points.ir.model.points.len(), 2);
    assert_eq!(points.ir.model.vertices.len(), 2);
    let native = points.ir.native.namespace("iges").unwrap();
    assert_eq!(native.arenas["copious_data"].len(), 1);
    assert_eq!(native.arenas["copious_data"][0].fields["tuples"][0][5], 1.0);
    assert!(points.report.losses.is_empty());

    let witness = IgesCodec
        .decode(
            &mut Cursor::new(copious_data_file(40, b"106,1,3,0,0,0,1,0,2,0;", "00000100")),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(!witness.report.geometry_transferred);
    assert!(witness.ir.model.curves.is_empty());
    assert!(witness.report.losses.is_empty());
    let validation = cadmpeg_ir::validate::validate(&witness.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

fn conic_arc_file(form: i64, parameters: &[u8]) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let parameter_count = parameters.len().div_ceil(64);
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["104", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        [
            "104",
            "0",
            "0",
            &parameter_count.to_string(),
            &form.to_string(),
            "",
            "",
            "CONIC",
            "0",
        ],
        2,
    ));
    bytes.extend(parameter_cards(parameters, 1, 1));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P{parameter_count:07}").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

#[test]
fn decode_classifies_and_bounds_all_standard_conic_arc_families() {
    let fixtures: [(i64, &[u8]); 4] = [
        (0, b"104,0.25,0,1,0,0,-1,0,2,0,0,1;"),
        (1, b"104,0.25,0,1,0,0,-1,0,2,0,0,1;"),
        (
            2,
            b"104,0.25,0,-0.1111111111111111,0,0,-1,0,2,0,3.086161269630487,3.525603580931404;",
        ),
        (3, b"104,1,0,0,0,-4,0,0,2,1,-2,1;"),
    ];
    for (form, parameters) in fixtures {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(conic_arc_file(form, parameters)),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert_eq!(result.ir.model.curves.len(), 1, "form {form}");
        assert_eq!(result.ir.model.edges.len(), 1, "form {form}");
        match (&result.ir.model.curves[0].geometry, form) {
            (cadmpeg_ir::geometry::CurveGeometry::Ellipse { .. }, 0 | 1)
            | (cadmpeg_ir::geometry::CurveGeometry::Hyperbola { .. }, 2)
            | (cadmpeg_ir::geometry::CurveGeometry::Parabola { .. }, 3) => {}
            (geometry, _) => panic!("unexpected form {form} geometry {geometry:?}"),
        }
        assert!(result.report.losses.is_empty(), "form {form}");
        let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
        assert!(
            validation.is_ok(),
            "form {form}: {:#?}",
            validation.findings
        );
    }
}

fn nurbs_curve_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["126", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        ["126", "0", "0", "1", "1", "", "", "NURBS", "0"],
        2,
    ));
    bytes.extend(parameter_card(
        b"126,1,1,1,0,1,0,0,0,1,1,1,1,0,0,0,2,0,0,0,1,0,0,1;",
        1,
        1,
    ));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn parametric_spline_curve_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let values = [
        "112", "3", "1", "3", "2", "0", "1", "2", // Header and breakpoints.
        "0", "1", "0", "0", "0", "0", "0", "0", "0", "0", "0", "0", // Segment 1.
        "1", "1", "0", "0", "0", "0", "0", "0", "0", "0", "0", "0", // Segment 2.
        "2", "1", "0", "0", "0", "0", "0", "0", "0", "0", "0", "0", // Terminal block.
    ];
    let parameters = format!("{};", values.join(","));
    let parameter_count = parameters.len().div_ceil(64);
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["112", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        [
            "112",
            "0",
            "0",
            &parameter_count.to_string(),
            "0",
            "",
            "",
            "SPLINE",
            "0",
        ],
        2,
    ));
    bytes.extend(parameter_cards(parameters.as_bytes(), 1, 1));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P{parameter_count:07}").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn parametric_spline_surface_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut values = vec![
        "114".to_owned(),
        "3".to_owned(),
        "1".to_owned(),
        "1".to_owned(),
        "1".to_owned(),
        "0".to_owned(),
        "1".to_owned(),
        "0".to_owned(),
        "1".to_owned(),
    ];
    let mut patch = vec!["0".to_owned(); 48];
    patch[1] = "1".into();
    patch[16 + 4] = "1".into();
    values.extend(patch);
    values.extend((0..48 * 3).map(|_| "0".to_owned()));
    let parameters = format!("{};", values.join(","));
    let parameter_count = parameters.len().div_ceil(64);
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["114", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        [
            "114",
            "0",
            "0",
            &parameter_count.to_string(),
            "0",
            "",
            "",
            "SPLSURF",
            "0",
        ],
        2,
    ));
    bytes.extend(parameter_cards(parameters.as_bytes(), 1, 1));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P{parameter_count:07}").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

#[test]
fn decode_converts_bicubic_power_patches_to_an_exact_nurbs_surface() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(parametric_spline_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(surface) =
        &result.ir.model.surfaces[0].geometry
    else {
        panic!("expected a bicubic NURBS carrier");
    };
    assert_eq!((surface.u_degree, surface.v_degree), (3, 3));
    assert_eq!((surface.u_count, surface.v_count), (4, 4));
    assert_eq!(
        cadmpeg_ir::eval::nurbs_surface_point(surface, 0.25, 0.75),
        Some(cadmpeg_ir::math::Point3::new(0.25, 0.75, 0.0))
    );
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_converts_piecewise_power_splines_to_exact_cubic_nurbs() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(parametric_spline_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &result.ir.model.curves[0].geometry
    else {
        panic!("expected a cubic NURBS carrier");
    };
    assert_eq!(nurbs.degree, 3);
    assert_eq!(
        nurbs.knots,
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0]
    );
    assert_eq!(nurbs.control_points.len(), 7);
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point(
            nurbs.degree,
            &nurbs.knots,
            &nurbs.control_points,
            None,
            1.5,
        ),
        Some(cadmpeg_ir::math::Point3::new(1.5, 0.0, 0.0))
    );
    assert_eq!(result.ir.model.edges[0].param_range, Some([0.0, 2.0]));
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

fn rational_nurbs_curve_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["126", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        ["126", "0", "0", "1", "0", "", "", "RNURBS", "0"],
        2,
    ));
    bytes.extend(parameter_card(
        b"126,2,2,1,0,0,0,0,0,0,1,1,1,1,0.5,1,0,0,0,1,1,0,2,0,0,0,1,0,0,1;",
        1,
        1,
    ));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn equal_weight_rational_nurbs_curve_file() -> Vec<u8> {
    let mut bytes = rational_nurbs_curve_file();
    let unequal = b",1,0.5,1,";
    let start = bytes
        .windows(unequal.len())
        .position(|window| window == unequal)
        .unwrap();
    bytes[start..start + unequal.len()].copy_from_slice(b",1,1.0,1,");
    bytes
}

fn nurbs_surface_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let parameters =
        b"128,1,1,1,1,0,0,1,0,0,0,0,1,1,0,0,1,1,1,1,1,1,0,0,0,1,0,0,0,1,0,1,1,0,0,1,0,1;";
    let parameter_count = parameters.len().div_ceil(64);
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["128", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        [
            "128",
            "0",
            "0",
            &parameter_count.to_string(),
            "0",
            "",
            "",
            "SURFACE",
            "0",
        ],
        2,
    ));
    bytes.extend(parameter_cards(parameters, 1, 1));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P{parameter_count:07}").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn ruled_surface_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, parameter_start, entity_type, form, label) in [
        (1, 1, "110", 0, "RAIL1"),
        (3, 2, "110", 0, "RAIL2"),
        (5, 3, "118", 1, "RULED"),
    ] {
        bytes.extend(directory_card(
            [
                entity_type,
                &parameter_start.to_string(),
                "0",
                "0",
                "0",
                "0",
                "0",
                "0",
                if entity_type == "110" {
                    "00010000"
                } else {
                    "00000000"
                },
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [
                entity_type,
                "0",
                "0",
                "1",
                &form.to_string(),
                "",
                "",
                label,
                "0",
            ],
            sequence + 1,
        ));
    }
    bytes.extend(parameter_card(b"110,0,0,0,1,0,0;", 1, 1));
    bytes.extend(parameter_card(b"110,0,1,0,1,1,0;", 3, 2));
    bytes.extend(parameter_card(b"118,1,3,0,1;", 5, 3));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000006P0000003").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

#[test]
fn decode_solves_a_parameter_matched_ruled_surface() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(ruled_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir.model.procedural_surfaces.len(), 1);
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(surface) =
        &result.ir.model.surfaces[0].geometry
    else {
        panic!("expected an exact NURBS ruled cache");
    };
    assert_eq!(
        cadmpeg_ir::eval::nurbs_surface_point(surface, 0.25, 0.75),
        Some(cadmpeg_ir::math::Point3::new(0.25, 0.75, 0.0))
    );
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

fn tabulated_cylinder_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, parameter_start, entity_type, label, status) in [
        (1, 1, "110", "DIRECTRX", "00010000"),
        (3, 2, "122", "TABULATE", "00000000"),
    ] {
        bytes.extend(directory_card(
            [
                entity_type,
                &parameter_start.to_string(),
                "0",
                "0",
                "0",
                "0",
                "0",
                "0",
                status,
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [entity_type, "0", "0", "1", "0", "", "", label, "0"],
            sequence + 1,
        ));
    }
    bytes.extend(parameter_card(b"110,0,0,0,1,0,0;", 1, 1));
    bytes.extend(parameter_card(b"122,1,0,0,2;", 3, 2));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000004P0000002").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn surface_of_revolution_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, parameter_start, entity_type, label, status) in [
        (1, 1, "110", "AXIS", "00010000"),
        (3, 2, "110", "PROFILE", "00010000"),
        (5, 3, "120", "REVOLVE", "00000000"),
    ] {
        bytes.extend(directory_card(
            [
                entity_type,
                &parameter_start.to_string(),
                "0",
                "0",
                "0",
                "0",
                "0",
                "0",
                status,
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [entity_type, "0", "0", "1", "0", "", "", label, "0"],
            sequence + 1,
        ));
    }
    bytes.extend(parameter_card(b"110,0,0,0,0,0,2;", 1, 1));
    bytes.extend(parameter_card(b"110,1,0,0,1,0,2;", 3, 2));
    bytes.extend(parameter_card(b"120,1,3,0,1.5707963267948966;", 5, 3));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000006P0000003").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn placed_surface_of_revolution_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, parameter_start, entity_type, transform, label, status) in [
        (1, 1, "110", "0", "AXIS", "00010000"),
        (3, 2, "110", "0", "PROFILE", "00010000"),
        (5, 3, "124", "0", "PLACE", "00010000"),
        (7, 4, "120", "5", "REVOLVE", "00000000"),
    ] {
        bytes.extend(directory_card(
            [
                entity_type,
                &parameter_start.to_string(),
                "0",
                "0",
                "0",
                "0",
                transform,
                "0",
                status,
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [entity_type, "0", "0", "1", "0", "", "", label, "0"],
            sequence + 1,
        ));
    }
    bytes.extend(parameter_card(b"110,0,0,0,0,0,2;", 1, 1));
    bytes.extend(parameter_card(b"110,1,0,0,1,0,2;", 3, 2));
    bytes.extend(parameter_card(b"124,1,0,0,10,0,1,0,0,0,0,1,0;", 5, 3));
    bytes.extend(parameter_card(b"120,1,3,0,1.5707963267948966;", 7, 4));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000008P0000004").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

#[test]
fn decode_solves_a_surface_of_revolution_as_rational_quadratic_spans() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(surface_of_revolution_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir.model.procedural_surfaces.len(), 1);
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(surface) =
        &result.ir.model.surfaces[0].geometry
    else {
        panic!("expected an exact rational revolution cache");
    };
    assert_eq!(surface.v_degree, 2);
    assert_eq!(surface.weights.as_ref().unwrap().len(), 6);
    let point =
        cadmpeg_ir::eval::nurbs_surface_point(surface, 0.5, std::f64::consts::FRAC_PI_4).unwrap();
    let expected = 0.5_f64.sqrt();
    assert!((point.x - expected).abs() < 1.0e-12);
    assert!((point.y - expected).abs() < 1.0e-12);
    assert!((point.z - 1.0).abs() < 1.0e-12);
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_places_a_surface_of_revolution_and_its_procedural_carriers_once() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(placed_surface_of_revolution_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(surface) =
        &result.ir.model.surfaces[0].geometry
    else {
        panic!("expected an exact rational revolution cache");
    };
    assert_eq!(surface.control_points[0].x, 11.0);
    let procedural = &result.ir.model.procedural_surfaces[0];
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution {
        directrix,
        axis_origin,
        ..
    } = &procedural.definition
    else {
        panic!("expected a revolution definition");
    };
    assert_eq!(axis_origin.x, 10.0);
    assert_eq!(directrix.0, "iges:model:curve#D7-placed-generatrix");
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_solves_a_tabulated_cylinder_as_an_exact_extrusion() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(tabulated_cylinder_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir.model.procedural_surfaces.len(), 1);
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(surface) =
        &result.ir.model.surfaces[0].geometry
    else {
        panic!("expected an exact NURBS extrusion cache");
    };
    assert_eq!(
        cadmpeg_ir::eval::nurbs_surface_point(surface, 0.5, 0.5),
        Some(cadmpeg_ir::math::Point3::new(0.5, 0.0, 1.0))
    );
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

fn plane_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["108", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        ["108", "0", "0", "1", "0", "", "", "PLANE", "0"],
        2,
    ));
    bytes.extend(parameter_card(b"108,0,0,1,2,0,0,0,2,0;", 1, 1));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

#[test]
fn decode_projects_an_unbounded_plane_from_implicit_coefficients() {
    let result = IgesCodec
        .decode(&mut Cursor::new(plane_file()), &DecodeOptions::default())
        .unwrap();

    let cadmpeg_ir::geometry::SurfaceGeometry::Plane {
        origin,
        normal,
        u_axis,
    } = &result.ir.model.surfaces[0].geometry
    else {
        panic!("expected a plane carrier");
    };
    assert_eq!(*origin, cadmpeg_ir::math::Point3::new(0.0, 0.0, 2.0));
    assert_eq!(*normal, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(*u_axis, cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(
        cadmpeg_ir::eval::surface_point(&result.ir.model.surfaces[0].geometry, 1.0, 3.0),
        Some(cadmpeg_ir::math::Point3::new(1.0, 3.0, 2.0))
    );
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

fn offset_plane_file(indicator_z: f64, distance: f64) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["108", "1", "0", "0", "0", "0", "0", "0", "00010000"],
        1,
    ));
    bytes.extend(directory_card(
        ["108", "0", "0", "1", "0", "", "", "PLANE", "0"],
        2,
    ));
    bytes.extend(directory_card(
        ["140", "2", "0", "0", "0", "0", "0", "0", "00000000"],
        3,
    ));
    bytes.extend(directory_card(
        ["140", "0", "0", "1", "0", "", "", "OFFSET", "0"],
        4,
    ));
    bytes.extend(parameter_card(b"108,0,0,1,0,0,0,0,0,0;", 1, 1));
    bytes.extend(parameter_card(
        format!("140,0,0,{indicator_z},{distance},1;").as_bytes(),
        3,
        2,
    ));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000004P0000002").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn pointer_defined_surface_file(entity_type: i64, form: i64) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, parameter_start, kind, label) in [
        (1, 1, 116, "LOCATION"),
        (3, 2, 123, "AXIS"),
        (5, 3, 123, "REFDIR"),
        (7, 4, entity_type, "SURFACE"),
    ] {
        let kind = kind.to_string();
        let parameter_start = parameter_start.to_string();
        let surface_form = form.to_string();
        bytes.extend(directory_card(
            [
                &kind,
                &parameter_start,
                "0",
                "0",
                "0",
                "0",
                "0",
                "0",
                if sequence == 7 {
                    "00000000"
                } else {
                    "00010000"
                },
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [
                &kind,
                "0",
                "0",
                "1",
                if sequence == 7 { &surface_form } else { "0" },
                "",
                "",
                label,
                "0",
            ],
            sequence + 1,
        ));
    }
    bytes.extend(parameter_card(b"116,1,2,3,0;", 1, 1));
    bytes.extend(parameter_card(b"123,0,0,1;", 3, 2));
    bytes.extend(parameter_card(b"123,1,0,0;", 5, 3));
    let parameters = match (entity_type, form) {
        (190, 0) => "190,1,3;",
        (190, 1) => "190,1,3,5;",
        (192, 0) => "192,1,3,2;",
        (192, 1) => "192,1,3,2,5;",
        (194, 0) => "194,1,3,2,30;",
        (194, 1) => "194,1,3,2,30,5;",
        (196, 0) => "196,1,2;",
        (196, 1) => "196,1,2,3,5;",
        (198, 0) => "198,1,3,4,1;",
        (198, 1) => "198,1,3,4,1,5;",
        _ => unreachable!(),
    };
    bytes.extend(parameter_card(parameters.as_bytes(), 7, 4));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000008P0000004").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn trimmed_plane_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, entity_type, form, label, status) in [
        (1_u32, 108, 0, "PLANE", "00010000"),
        (3, 106, 63, "MODEL", "00010000"),
        (5, 106, 63, "PCURVE", "00010500"),
        (7, 142, 0, "ON_SURF", "00010000"),
        (9, 144, 0, "TRIMMED", "00000000"),
    ] {
        let entity_type = entity_type.to_string();
        let parameter_start = sequence.div_ceil(2).to_string();
        let form = form.to_string();
        bytes.extend(directory_card(
            [
                &entity_type,
                &parameter_start,
                "0",
                "0",
                "0",
                "0",
                "0",
                "0",
                status,
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [&entity_type, "0", "0", "1", &form, "", "", label, "0"],
            sequence + 1,
        ));
    }
    bytes.extend(parameter_card(b"108,0,0,1,0,0,0,0,0,0;", 1, 1));
    let square = b"106,1,5,0,0,0,1,0,1,1,0,1,0,0;";
    bytes.extend(parameter_card(square, 3, 2));
    bytes.extend(parameter_card(square, 5, 3));
    bytes.extend(parameter_card(b"142,0,1,5,3,3;", 7, 4));
    bytes.extend(parameter_card(b"144,1,1,0,7;", 9, 5));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000010P0000005").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn model_curve_only_trimmed_plane_file() -> Vec<u8> {
    let mut bytes = trimmed_plane_file();
    let parameter = b"142,0,1,5,3,3;";
    let start = bytes
        .windows(parameter.len())
        .position(|window| window == parameter)
        .unwrap();
    bytes[start..start + parameter.len()].copy_from_slice(b"142,0,1,0,3,2;");
    bytes
}

fn bounded_plane_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, entity_type, label, status) in [
        (1_u32, 108, "PLANE", "00010000"),
        (3, 110, "EDGE1", "00010000"),
        (5, 110, "EDGE2", "00010000"),
        (7, 110, "EDGE3", "00010000"),
        (9, 110, "EDGE4", "00010000"),
        (11, 141, "BOUNDARY", "00010000"),
        (13, 143, "BOUNDED", "00000000"),
    ] {
        let entity_type = entity_type.to_string();
        let parameter_start = sequence.div_ceil(2).to_string();
        bytes.extend(directory_card(
            [
                &entity_type,
                &parameter_start,
                "0",
                "0",
                "0",
                "0",
                "0",
                "0",
                status,
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [&entity_type, "0", "0", "1", "0", "", "", label, "0"],
            sequence + 1,
        ));
    }
    for (sequence, parameter_sequence, parameters) in [
        (1, 1, "108,0,0,1,0,0,0,0,0,0;"),
        (3, 2, "110,0,0,0,1,0,0;"),
        (5, 3, "110,1,1,0,1,0,0;"),
        (7, 4, "110,1,1,0,0,1,0;"),
        (9, 5, "110,0,1,0,0,0,0;"),
        (11, 6, "141,0,1,1,4,3,1,0,5,2,0,7,1,0,9,1,0;"),
        (13, 7, "143,0,1,1,11;"),
    ] {
        bytes.extend(parameter_card(
            parameters.as_bytes(),
            sequence,
            parameter_sequence,
        ));
    }
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000014P0000007").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn parametrically_bounded_plane_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, entity_type, form, label, status) in [
        (1_u32, 108, 0, "PLANE", "00010000"),
        (3, 106, 63, "MODEL", "00010000"),
        (5, 106, 63, "PCURVE", "00010500"),
        (7, 141, 0, "BOUNDARY", "00010000"),
        (9, 143, 0, "BOUNDED", "00000000"),
    ] {
        let entity_type = entity_type.to_string();
        let parameter_start = sequence.div_ceil(2).to_string();
        let form = form.to_string();
        bytes.extend(directory_card(
            [
                &entity_type,
                &parameter_start,
                "0",
                "0",
                "0",
                "0",
                "0",
                "0",
                status,
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [&entity_type, "0", "0", "1", &form, "", "", label, "0"],
            sequence + 1,
        ));
    }
    bytes.extend(parameter_card(b"108,0,0,1,0,0,0,0,0,0;", 1, 1));
    let square = b"106,1,5,0,0,0,1,0,1,1,0,1,0,0;";
    bytes.extend(parameter_card(square, 3, 2));
    bytes.extend(parameter_card(square, 5, 3));
    bytes.extend(parameter_card(b"141,1,3,1,1,3,1,1,5;", 7, 4));
    bytes.extend(parameter_card(b"143,1,1,1,7;", 9, 5));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000010P0000005").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn explicit_open_shell_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, entity_type, form, label, status) in [
        (1_u32, 116, 0, "LOCATION", "00010000"),
        (3, 123, 0, "NORMAL", "00010000"),
        (5, 190, 0, "SURFACE", "00010000"),
        (7, 110, 0, "EDGE1", "00010000"),
        (9, 110, 0, "EDGE2", "00010000"),
        (11, 110, 0, "EDGE3", "00010000"),
        (13, 110, 0, "EDGE4", "00010000"),
        (15, 502, 1, "VERTICES", "00010000"),
        (17, 504, 1, "EDGES", "00010001"),
        (19, 508, 1, "LOOP", "00010000"),
        (21, 510, 1, "FACE", "00010000"),
        (23, 514, 2, "SHELL", "00000000"),
    ] {
        let entity_type = entity_type.to_string();
        let parameter_start = sequence.div_ceil(2).to_string();
        let form = form.to_string();
        bytes.extend(directory_card(
            [
                &entity_type,
                &parameter_start,
                "0",
                "0",
                "0",
                "0",
                "0",
                "0",
                status,
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [&entity_type, "0", "0", "1", &form, "", "", label, "0"],
            sequence + 1,
        ));
    }
    for (sequence, parameter_sequence, parameters) in [
        (1, 1, "116,0,0,0,0;"),
        (3, 2, "123,0,0,1;"),
        (5, 3, "190,1,3;"),
        (7, 4, "110,0,0,0,1,0,0;"),
        (9, 5, "110,1,0,0,1,1,0;"),
        (11, 6, "110,1,1,0,0,1,0;"),
        (13, 7, "110,0,1,0,0,0,0;"),
        (15, 8, "502,4,0,0,0,1,0,0,1,1,0,0,1,0;"),
        (
            17,
            9,
            "504,4,7,15,1,15,2,9,15,2,15,3,11,15,3,15,4,13,15,4,15,1;",
        ),
        (19, 10, "508,4,0,17,1,1,0,0,17,2,1,0,0,17,3,1,0,0,17,4,1,0;"),
        (21, 11, "510,5,1,1,19;"),
        (23, 12, "514,1,21,1;"),
    ] {
        bytes.extend(parameter_card(
            parameters.as_bytes(),
            sequence,
            parameter_sequence,
        ));
    }
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000024P0000012").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn explicit_non_manifold_open_shell_file() -> Vec<u8> {
    let mut entities = vec![
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "LOCATION".into(),
            status: "00010000",
            parameters: "116,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 123,
            form: 0,
            label: "NORMAL".into(),
            status: "00010000",
            parameters: "123,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 190,
            form: 0,
            label: "SURFACE".into(),
            status: "00010000",
            parameters: "190,1,3;".into(),
        },
    ];
    for (index, parameters) in [
        "110,0,0,0,1,0,0;",
        "110,1,0,0,0,1,0;",
        "110,0,1,0,0,0,0;",
        "110,0,0,0,0,-1,0;",
        "110,0,-1,0,1,0,0;",
        "110,1,0,0,0.5,1,0;",
        "110,0.5,1,0,0,0,0;",
    ]
    .into_iter()
    .enumerate()
    {
        entities.push(OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: format!("EDGE{}", index + 1),
            status: "00010000",
            parameters: parameters.into(),
        });
    }
    entities.extend([
        OwnedTestEntity {
            entity_type: 502,
            form: 1,
            label: "VERTICES".into(),
            status: "00010000",
            parameters: "502,5,0,0,0,1,0,0,0,1,0,0,-1,0,0.5,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 504,
            form: 1,
            label: "EDGES".into(),
            status: "00010001",
            parameters: "504,7,7,21,1,21,2,9,21,2,21,3,11,21,3,21,1,13,21,1,21,4,15,21,4,21,2,17,21,2,21,5,19,21,5,21,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 508,
            form: 1,
            label: "LOOP1".into(),
            status: "00010000",
            parameters: "508,3,0,23,1,1,0,0,23,2,1,0,0,23,3,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 508,
            form: 1,
            label: "LOOP2".into(),
            status: "00010000",
            parameters: "508,3,0,23,1,0,0,0,23,4,1,0,0,23,5,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 508,
            form: 1,
            label: "LOOP3".into(),
            status: "00010000",
            parameters: "508,3,0,23,1,1,0,0,23,6,1,0,0,23,7,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 510,
            form: 1,
            label: "FACE1".into(),
            status: "00010000",
            parameters: "510,5,1,1,25;".into(),
        },
        OwnedTestEntity {
            entity_type: 510,
            form: 1,
            label: "FACE2".into(),
            status: "00010000",
            parameters: "510,5,1,1,27;".into(),
        },
        OwnedTestEntity {
            entity_type: 510,
            form: 1,
            label: "FACE3".into(),
            status: "00010000",
            parameters: "510,5,1,1,29;".into(),
        },
        OwnedTestEntity {
            entity_type: 514,
            form: 2,
            label: "SHELL".into(),
            status: "00000000",
            parameters: "514,3,31,1,33,1,35,1;".into(),
        },
    ]);
    owned_test_file(&entities)
}

fn explicit_tetrahedron_solid_file() -> Vec<u8> {
    explicit_tetrahedron_solid_file_with_options(false, false)
}

fn explicit_tetrahedron_solid_file_with_transform(transformed: bool) -> Vec<u8> {
    explicit_tetrahedron_solid_file_with_options(transformed, false)
}

fn explicit_tetrahedron_solid_file_with_options(
    transformed: bool,
    inconsistent_radial_sense: bool,
) -> Vec<u8> {
    explicit_tetrahedron_solid_file_extended(transformed, inconsistent_radial_sense, false)
}

fn explicit_tetrahedron_solid_with_boolean_file() -> Vec<u8> {
    explicit_tetrahedron_solid_file_extended(false, false, true)
}

fn explicit_tetrahedron_solid_file_extended(
    transformed: bool,
    inconsistent_radial_sense: bool,
    with_boolean: bool,
) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut entities = vec![
        (116, 0, "POINTA", "00010000", "116,0,0,0,0;"),
        (116, 0, "POINTB", "00010000", "116,1,0,0,0;"),
        (123, 0, "NEGZ", "00010000", "123,0,0,-1;"),
        (123, 0, "NEGY", "00010000", "123,0,-1,0;"),
        (123, 0, "NEGX", "00010000", "123,-1,0,0;"),
        (
            123,
            0,
            "DIAG",
            "00010000",
            "123,0.5773502691896258,0.5773502691896258,0.5773502691896258;",
        ),
        (190, 0, "SURF1", "00010000", "190,1,5;"),
        (190, 0, "SURF2", "00010000", "190,1,7;"),
        (190, 0, "SURF3", "00010000", "190,1,9;"),
        (190, 0, "SURF4", "00010000", "190,3,11;"),
        (110, 0, "AB", "00010000", "110,0,0,0,1,0,0;"),
        (110, 0, "AC", "00010000", "110,0,0,0,0,1,0;"),
        (110, 0, "AD", "00010000", "110,0,0,0,0,0,1;"),
        (110, 0, "BC", "00010000", "110,1,0,0,0,1,0;"),
        (110, 0, "BD", "00010000", "110,1,0,0,0,0,1;"),
        (110, 0, "CD", "00010000", "110,0,1,0,0,0,1;"),
        (
            502,
            1,
            "VERTICES",
            "00010000",
            "502,4,0,0,0,1,0,0,0,1,0,0,0,1;",
        ),
        (
            504,
            1,
            "EDGES",
            "00010001",
            "504,6,21,33,1,33,2,23,33,1,33,3,25,33,1,33,4,27,33,2,33,3,29,33,2,33,4,31,33,3,33,4;",
        ),
        (
            508,
            1,
            "LOOP1",
            "00010000",
            "508,3,0,35,2,1,0,0,35,4,0,0,0,35,1,0,0;",
        ),
        (
            508,
            1,
            "LOOP2",
            "00010000",
            "508,3,0,35,1,1,0,0,35,5,1,0,0,35,3,0,0;",
        ),
        (
            508,
            1,
            "LOOP3",
            "00010000",
            "508,3,0,35,3,1,0,0,35,6,0,0,0,35,2,0,0;",
        ),
        if inconsistent_radial_sense {
            (
                508,
                1,
                "LOOP4",
                "00010000",
                "508,3,0,35,4,1,0,0,35,6,0,0,0,35,5,0,0;",
            )
        } else {
            (
                508,
                1,
                "LOOP4",
                "00010000",
                "508,3,0,35,4,1,0,0,35,6,1,0,0,35,5,0,0;",
            )
        },
        (510, 1, "FACE1", "00010000", "510,13,1,1,37;"),
        (510, 1, "FACE2", "00010000", "510,15,1,1,39;"),
        (510, 1, "FACE3", "00010000", "510,17,1,1,41;"),
        (510, 1, "FACE4", "00010000", "510,19,1,1,43;"),
        (514, 1, "SHELL", "00010000", "514,4,45,1,47,1,49,1,51,1;"),
        (186, 0, "SOLID", "00000000", "186,53,1,0;"),
    ];
    if transformed {
        entities.push((
            124,
            0,
            "PLACE",
            "00010000",
            "124,1,0,0,10,0,1,0,20,0,0,1,30;",
        ));
    }
    if with_boolean {
        entities.extend([
            (158, 0, "SPHERE", "00000000", "158,1,2,2,2;"),
            (180, 1, "MIXED", "00000000", "180,3,-55,-57,1;"),
            (184, 1, "ASSEMBLY", "00000200", "184,2,55,57,0,0;"),
            (430, 1, "BREPINST", "00000000", "430,55;"),
        ]);
    }
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    let mut parameter_sequence = 1_u32;
    for (index, (entity_type, form, label, status, parameters)) in entities.iter().enumerate() {
        let sequence = u32::try_from(index * 2 + 1).unwrap();
        let line_count = parameters.len().div_ceil(64);
        let entity_type = entity_type.to_string();
        let form = form.to_string();
        let parameter_start = parameter_sequence.to_string();
        let line_count_string = line_count.to_string();
        let transform = if transformed && entity_type == "186" {
            "57"
        } else {
            "0"
        };
        bytes.extend(directory_card(
            [
                &entity_type,
                &parameter_start,
                "0",
                "0",
                "0",
                "0",
                transform,
                "0",
                status,
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [
                &entity_type,
                "0",
                "0",
                &line_count_string,
                &form,
                "",
                "",
                label,
                "0",
            ],
            sequence + 1,
        ));
        parameter_sequence += u32::try_from(line_count).unwrap();
    }
    parameter_sequence = 1;
    for (index, (_, _, _, _, parameters)) in entities.iter().enumerate() {
        let sequence = u32::try_from(index * 2 + 1).unwrap();
        bytes.extend(parameter_cards(
            parameters.as_bytes(),
            sequence,
            parameter_sequence,
        ));
        parameter_sequence += u32::try_from(parameters.len().div_ceil(64)).unwrap();
    }
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!(
            "S0000001G{global_cards:07}D{:07}P{:07}",
            entities.len() * 2,
            parameter_sequence - 1
        )
        .as_bytes(),
        b'T',
        1,
    ));
    bytes
}

struct OwnedTestEntity {
    entity_type: i64,
    form: i64,
    label: String,
    status: &'static str,
    parameters: String,
}

fn owned_test_file(entities: &[OwnedTestEntity]) -> Vec<u8> {
    owned_test_file_with_colors(entities, &[])
}

fn owned_test_file_with_colors(entities: &[OwnedTestEntity], colors: &[(u32, i64)]) -> Vec<u8> {
    owned_test_file_with_display(entities, colors, &[])
}

fn owned_test_file_with_display(
    entities: &[OwnedTestEntity],
    colors: &[(u32, i64)],
    line_fonts: &[(u32, i64)],
) -> Vec<u8> {
    owned_test_file_with_attributes(entities, colors, line_fonts, &[], &[])
}

fn owned_test_file_with_levels(entities: &[OwnedTestEntity], levels: &[(u32, i64)]) -> Vec<u8> {
    owned_test_file_with_attributes(entities, &[], &[], levels, &[])
}

fn owned_test_file_with_line_weights(
    entities: &[OwnedTestEntity],
    line_weights: &[(u32, i64)],
) -> Vec<u8> {
    owned_test_file_with_attributes(entities, &[], &[], &[], line_weights)
}

fn owned_test_file_with_attributes(
    entities: &[OwnedTestEntity],
    colors: &[(u32, i64)],
    line_fonts: &[(u32, i64)],
    levels: &[(u32, i64)],
    line_weights: &[(u32, i64)],
) -> Vec<u8> {
    owned_test_file_with_directory_fields(entities, colors, line_fonts, levels, line_weights, &[])
}

fn owned_test_file_with_structures(
    entities: &[OwnedTestEntity],
    structures: &[(u32, i64)],
) -> Vec<u8> {
    owned_test_file_with_directory_fields(entities, &[], &[], &[], &[], structures)
}

fn owned_test_file_with_directory_fields(
    entities: &[OwnedTestEntity],
    colors: &[(u32, i64)],
    line_fonts: &[(u32, i64)],
    levels: &[(u32, i64)],
    line_weights: &[(u32, i64)],
    structures: &[(u32, i64)],
) -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    let mut parameter_sequence = 1_u32;
    for (index, entity) in entities.iter().enumerate() {
        let sequence = u32::try_from(index * 2 + 1).unwrap();
        let line_count = entity.parameters.len().div_ceil(64);
        bytes.extend(directory_card(
            [
                &entity.entity_type.to_string(),
                &parameter_sequence.to_string(),
                &structures
                    .iter()
                    .find_map(|(entry, structure)| (*entry == sequence).then_some(*structure))
                    .unwrap_or(0)
                    .to_string(),
                &line_fonts
                    .iter()
                    .find_map(|(entry, line_font)| (*entry == sequence).then_some(*line_font))
                    .unwrap_or(0)
                    .to_string(),
                &levels
                    .iter()
                    .find_map(|(entry, level)| (*entry == sequence).then_some(*level))
                    .unwrap_or(0)
                    .to_string(),
                "0",
                "0",
                "0",
                entity.status,
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [
                &entity.entity_type.to_string(),
                &line_weights
                    .iter()
                    .find_map(|(entry, weight)| (*entry == sequence).then_some(*weight))
                    .unwrap_or(0)
                    .to_string(),
                &colors
                    .iter()
                    .find_map(|(entry, color)| (*entry == sequence).then_some(*color))
                    .unwrap_or(0)
                    .to_string(),
                &line_count.to_string(),
                &entity.form.to_string(),
                "",
                "",
                &entity.label,
                "0",
            ],
            sequence + 1,
        ));
        parameter_sequence += u32::try_from(line_count).unwrap();
    }
    parameter_sequence = 1;
    for (index, entity) in entities.iter().enumerate() {
        let sequence = u32::try_from(index * 2 + 1).unwrap();
        bytes.extend(parameter_cards(
            entity.parameters.as_bytes(),
            sequence,
            parameter_sequence,
        ));
        parameter_sequence += u32::try_from(entity.parameters.len().div_ceil(64)).unwrap();
    }
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!(
            "S0000001G{global_cards:07}D{:07}P{:07}",
            entities.len() * 2,
            parameter_sequence - 1
        )
        .as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn explicit_vertex_loop_file() -> Vec<u8> {
    explicit_vertex_loop_file_with_outer_flag(true)
}

fn explicit_vertex_loop_file_with_outer_flag(has_outer_loop: bool) -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "CENTER".into(),
            status: "00010000",
            parameters: "116,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 196,
            form: 0,
            label: "SPHERE".into(),
            status: "00010000",
            parameters: "196,1,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 502,
            form: 1,
            label: "POLE".into(),
            status: "00010000",
            parameters: "502,1,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 508,
            form: 1,
            label: "VLOOP".into(),
            status: "00010000",
            parameters: "508,1,1,5,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 510,
            form: 1,
            label: "FACE".into(),
            status: "00010000",
            parameters: format!("510,3,1,{},7;", i32::from(has_outer_loop)),
        },
        OwnedTestEntity {
            entity_type: 514,
            form: 2,
            label: "SHELL".into(),
            status: "00000000",
            parameters: "514,1,9,1;".into(),
        },
    ])
}

fn colored_explicit_vertex_loop_file() -> Vec<u8> {
    let entities = [
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "CENTER".into(),
            status: "00010000",
            parameters: "116,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 196,
            form: 0,
            label: "SPHERE".into(),
            status: "00010000",
            parameters: "196,1,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 502,
            form: 1,
            label: "POLE".into(),
            status: "00010000",
            parameters: "502,1,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 508,
            form: 1,
            label: "VLOOP".into(),
            status: "00010000",
            parameters: "508,1,1,5,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 510,
            form: 1,
            label: "FACE".into(),
            status: "00010000",
            parameters: "510,3,1,1,7;".into(),
        },
        OwnedTestEntity {
            entity_type: 514,
            form: 2,
            label: "SHELL".into(),
            status: "00000000",
            parameters: "514,1,9,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 314,
            form: 0,
            label: "COLOR".into(),
            status: "00000200",
            parameters: "314,20,40,60,6Hcustom;".into(),
        },
    ];
    owned_test_file_with_colors(&entities, &[(9, -13), (11, 2), (13, 2)])
}

fn line_font_definitions_file() -> Vec<u8> {
    let entities = [
        OwnedTestEntity {
            entity_type: 308,
            form: 0,
            label: "TEMPLATE".into(),
            status: "00000200",
            parameters: "308,0,4HMARK,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 304,
            form: 1,
            label: "SYMBOLS".into(),
            status: "00000200",
            parameters: "304,1,1,2,0.5;".into(),
        },
        OwnedTestEntity {
            entity_type: 304,
            form: 2,
            label: "PATTERN".into(),
            status: "00000200",
            parameters: "304,5,2,1,2,1,2,2H16;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "LINE".into(),
            status: "00000000",
            parameters: "110,0,0,0,1,0,0;".into(),
        },
    ];
    owned_test_file_with_display(&entities, &[], &[(3, 1), (5, 2), (7, -5)])
}

fn definition_levels_file() -> Vec<u8> {
    let entities = [
        OwnedTestEntity {
            entity_type: 406,
            form: 1,
            label: "LEVELS".into(),
            status: "00000200",
            parameters: "406,3,2,7,11;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "LINE".into(),
            status: "00000000",
            parameters: "110,0,0,0,1,0,0;".into(),
        },
    ];
    owned_test_file_with_levels(&entities, &[(3, -1)])
}

fn weighted_line_file() -> Vec<u8> {
    let entities = [OwnedTestEntity {
        entity_type: 110,
        form: 0,
        label: "LINE".into(),
        status: "00000000",
        parameters: "110,0,0,0,1,0,0;".into(),
    }];
    owned_test_file_with_line_weights(&entities, &[(1, 1)])
}

fn primitive_solids_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 150,
            form: 0,
            label: "BLOCK".into(),
            status: "00000000",
            parameters: "150,2,3,4,1,2,3,1,0,0,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 150,
            form: 0,
            label: "DEFAULT".into(),
            status: "00000000",
            parameters: "150,1,2,3,,,,,,,,,;".into(),
        },
        OwnedTestEntity {
            entity_type: 152,
            form: 0,
            label: "WEDGE".into(),
            status: "00000000",
            parameters: "152,4,3,2,1,0,0,0,1,0,0,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 154,
            form: 0,
            label: "CYLINDER".into(),
            status: "00000000",
            parameters: "154,5,2,1,2,3,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 156,
            form: 0,
            label: "FRUSTUM".into(),
            status: "00000000",
            parameters: "156,5,3,1,1,2,3,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 158,
            form: 0,
            label: "SPHERE".into(),
            status: "00000000",
            parameters: "158,2,1,2,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 160,
            form: 0,
            label: "TORUS".into(),
            status: "00000000",
            parameters: "160,4,1,1,2,3,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 168,
            form: 0,
            label: "ELLIPSO".into(),
            status: "00000000",
            parameters: "168,4,3,2,1,2,3,1,0,0,0,0,1;".into(),
        },
    ])
}

fn procedural_and_boolean_solids_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "PROFILE1".into(),
            status: "00010000",
            parameters: "110,1,0,0,2,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 100,
            form: 0,
            label: "PROFILE2".into(),
            status: "00010000",
            parameters: "100,0,0,0,1,0,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 162,
            form: 0,
            label: "REVOPEN".into(),
            status: "00000000",
            parameters: "162,1,0.5,0,0,0,0,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 162,
            form: 1,
            label: "REVCLOSE".into(),
            status: "00000000",
            parameters: "162,3,1,0,0,0,0,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 164,
            form: 0,
            label: "EXTRUDE".into(),
            status: "00000000",
            parameters: "164,3,5,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 158,
            form: 0,
            label: "SPHERE1".into(),
            status: "00000000",
            parameters: "158,2,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 158,
            form: 0,
            label: "SPHERE2".into(),
            status: "00000000",
            parameters: "158,1,3,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 180,
            form: 0,
            label: "UNION".into(),
            status: "00000000",
            parameters: "180,3,-11,-13,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 182,
            form: 0,
            label: "SELECT".into(),
            status: "00000300",
            parameters: "182,15,1,0,0;".into(),
        },
    ])
}

fn solid_assembly_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 158,
            form: 0,
            label: "SPHERE1".into(),
            status: "00000000",
            parameters: "158,1,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 158,
            form: 0,
            label: "SPHERE2".into(),
            status: "00000000",
            parameters: "158,1,3,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 124,
            form: 0,
            label: "MOVE".into(),
            status: "00010000",
            parameters: "124,1,0,0,10,0,1,0,0,0,0,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 184,
            form: 0,
            label: "ASSEMBLY".into(),
            status: "00000200",
            parameters: "184,2,1,3,0,5;".into(),
        },
    ])
}

fn solid_instance_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 158,
            form: 0,
            label: "SPHERE".into(),
            status: "00000000",
            parameters: "158,2,1,2,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 430,
            form: 0,
            label: "INSTANCE".into(),
            status: "00000000",
            parameters: "430,1;".into(),
        },
    ])
}

fn patterned_instance_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "BASE".into(),
            status: "00000000",
            parameters: "116,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 412,
            form: 0,
            label: "RECT".into(),
            status: "00000000",
            parameters: "412,1,2,1,2,3,2,3,10,5,0.25,1,0,2;".into(),
        },
        OwnedTestEntity {
            entity_type: 414,
            form: 0,
            label: "CIRCLE".into(),
            status: "00000000",
            parameters: "414,3,4,10,20,30,8,0.5,1.25,2,1,1,3;".into(),
        },
    ])
}

fn external_reference_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 416,
            form: 0,
            label: "EXTDEF".into(),
            status: "00000000",
            parameters: "416,8Hpart.igs,7HBRACKET;".into(),
        },
        OwnedTestEntity {
            entity_type: 416,
            form: 1,
            label: "EXTFILE".into(),
            status: "00000000",
            parameters: "416,12Hassembly.igs;".into(),
        },
        OwnedTestEntity {
            entity_type: 416,
            form: 2,
            label: "EXTLOGIC".into(),
            status: "00000000",
            parameters: "416,9Hsheet.igs,7HFLANGE1;".into(),
        },
        OwnedTestEntity {
            entity_type: 416,
            form: 3,
            label: "NATIVE".into(),
            status: "00000000",
            parameters: "416,5HMOTOR;".into(),
        },
        OwnedTestEntity {
            entity_type: 416,
            form: 4,
            label: "LIBRARY".into(),
            status: "00000000",
            parameters: "416,7HDEVICES,5HRELAY;".into(),
        },
    ])
}

fn group_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "ORDERED".into(),
            status: "00000000",
            parameters: "116,1,2,3,0,1,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 14,
            label: "GROUP1".into(),
            status: "00000000",
            parameters: "402,1,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "UNORDER".into(),
            status: "00000000",
            parameters: "116,4,5,6,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 7,
            label: "GROUP2".into(),
            status: "00000000",
            parameters: "402,1,5;".into(),
        },
    ])
}

fn attribute_definition_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 322,
            form: 0,
            label: "ATTRDEF".into(),
            status: "00000000",
            parameters: "322,4HMETA,1,2,10,1,1,11,3,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 322,
            form: 1,
            label: "ATTRROW".into(),
            status: "00000000",
            parameters: "322,4HROW1,1,2,10,1,1,42,11,3,1,5HSTEEL;".into(),
        },
        OwnedTestEntity {
            entity_type: 322,
            form: 2,
            label: "ATTRDSP".into(),
            status: "00000000",
            parameters: "322,4HROW2,1,2,10,2,1,3.5,0,11,6,1,1,0;".into(),
        },
    ])
}

fn attribute_instance_forms_file() -> Vec<u8> {
    let entities = [
        OwnedTestEntity {
            entity_type: 322,
            form: 0,
            label: "ATTRDEF".into(),
            status: "00000000",
            parameters: "322,4HMETA,1,2,10,1,1,11,3,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 422,
            form: 0,
            label: "ATTRONE".into(),
            status: "00000000",
            parameters: "422,7,5HSTEEL;".into(),
        },
        OwnedTestEntity {
            entity_type: 422,
            form: 1,
            label: "ATTRTAB".into(),
            status: "00000000",
            parameters: "422,2,8,4HIRON,9,5HBRASS;".into(),
        },
    ];
    owned_test_file_with_structures(&entities, &[(3, -1), (5, -1)])
}

fn product_property_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "COMP".into(),
            status: "00000000",
            parameters: "116,0,0,0,0,0,2,3,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 7,
            label: "REFDES".into(),
            status: "00010000",
            parameters: "406,1,2HR1;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 15,
            label: "NAME".into(),
            status: "00010000",
            parameters: "406,1,7HBRACKET;".into(),
        },
    ])
}

fn scalar_property_forms_file() -> Vec<u8> {
    let cases = [
        (2, "406,3,0,1,2;"),
        (3, "406,2,17,5HPOWER;"),
        (5, "406,5,0.25,0,2,1,0.1;"),
        (6, "406,5,0.5,0.45,1,2,8;"),
        (8, "406,1,3HPA7;"),
        (9, "406,4,7HGENERIC,6HMIL123,6HVEND42,5HINT99;"),
        (10, "406,6,1,0,1,0,1,0;"),
        (12, "406,2,8HBASE.IGS,10HDETAIL.IGS;"),
        (13, "406,3,2.5,3HAWG,7HANSI123;"),
        (14, "406,2,4HMAIN,3HHOT;"),
        (18, "406,1,12.5;"),
        (19, "406,1,223;"),
        (20, "406,1,1;"),
        (21, "406,1,0;"),
    ];
    let entities = cases
        .into_iter()
        .map(|(form, parameters)| OwnedTestEntity {
            entity_type: 406,
            form,
            label: format!("PROP{form}"),
            status: "00000000",
            parameters: parameters.into(),
        })
        .collect::<Vec<_>>();
    owned_test_file(&entities)
}

fn grid_property_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW".into(),
            status: "00020000",
            parameters: "410,1,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 22,
            label: "GRID".into(),
            status: "00010000",
            parameters: "406,9,1,1,0,0,0,5,10,20,30;".into(),
        },
        OwnedTestEntity {
            entity_type: 404,
            form: 1,
            label: "DRAWING".into(),
            status: "00000000",
            parameters: "404,1,1,0,0,0,0,0,1,3;".into(),
        },
    ])
}

fn group_type_property_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "ITEM".into(),
            status: "00000000",
            parameters: "116,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 7,
            label: "GROUP".into(),
            status: "00000000",
            parameters: "402,1,1,0,1,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 23,
            label: "GROUPTYP".into(),
            status: "00010000",
            parameters: "406,2,5,5HDRILL;".into(),
        },
    ])
}

fn lep_property_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 406,
            form: 24,
            label: "LAYERMAP".into(),
            status: "00000000",
            parameters: "406,9,2,10,4HTOP1,1,8HSIGNAL_T,20,4HCORE,0,9HUNDEFINED;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 25,
            label: "STACKUP".into(),
            status: "00000000",
            parameters: "406,5,5HBOARD,3,10,20,30;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "HOLE".into(),
            status: "00000000",
            parameters: "116,0,0,0,0,1,7;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 26,
            label: "DRILL".into(),
            status: "00010000",
            parameters: "406,3,0.8,0.7,5;".into(),
        },
    ])
}

fn variable_schema_property_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "OWNER".into(),
            status: "00000000",
            parameters: "116,0,0,0,0,1,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 27,
            label: "GENERIC".into(),
            status: "00010000",
            parameters: "406,14,4HMETA,6,0,,1,42,2,3.5,3,5HSTEEL,4,1,6,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 11,
            label: "TABULAR".into(),
            status: "00000000",
            parameters: "406,9,5,1,1,2,2,50,25,33,46;".into(),
        },
    ])
}

fn dimension_property_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "DIMNOTE".into(),
            status: "00010100",
            parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
        },
        OwnedTestEntity {
            entity_type: 214,
            form: 1,
            label: "ARROW".into(),
            status: "00010100",
            parameters: "214,1,2,1,0,0,0,2,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 216,
            form: 0,
            label: "DIMENS".into(),
            status: "00000100",
            parameters: "216,1,3,3,0,0,0,4,7,9,11,13;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 28,
            label: "DIMUNITS".into(),
            status: "00000000",
            parameters: "406,6,0,2,1,2HMM,0,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 29,
            label: "DIMTOL".into(),
            status: "00000000",
            parameters: "406,8,0,2,2,0.1,-0.1,0,0,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 31,
            label: "BASICDIM".into(),
            status: "00010000",
            parameters: "406,8,0,0,2,0,2,1,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 30,
            label: "DIMDISP".into(),
            status: "00010000",
            parameters: "406,15,2,1,1,3HDIA,0,1.5707963267948966,1,0,0,0,12.5,1,1,1,1;".into(),
        },
    ])
}

fn drawing_metadata_property_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW".into(),
            status: "00020000",
            parameters: "410,1,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 32,
            label: "APPROVAL".into(),
            status: "00000000",
            parameters: "406,3,4HJANE,3HENG,15H20260714.123456;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 33,
            label: "SHEETID".into(),
            status: "00000000",
            parameters: "406,2,2,1HC;".into(),
        },
        OwnedTestEntity {
            entity_type: 404,
            form: 1,
            label: "DRAWING".into(),
            status: "00000000",
            parameters: "404,1,1,0,0,0,0,0,2,3,5;".into(),
        },
    ])
}

fn text_score_property_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "NOTE".into(),
            status: "00010100",
            parameters: "212,1,5,1,1,1,1.5707963267948966,0,0,0,0,0,0,5HABCDE,0,2,3,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 34,
            label: "UNDER".into(),
            status: "00010000",
            parameters: "406,4,1,1,2,4;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 35,
            label: "OVER".into(),
            status: "00010000",
            parameters: "406,4,1,1,3,5;".into(),
        },
    ])
}

fn closure_property_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 100,
            form: 0,
            label: "CIRCLE".into(),
            status: "00000000",
            parameters: "100,0,0,0,1,0,0,1,0,1,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 36,
            label: "CLOSURE".into(),
            status: "00010000",
            parameters: "406,1,2;".into(),
        },
    ])
}

fn view_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "ORTHO".into(),
            status: "00000000",
            parameters: "410,1,,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 410,
            form: 1,
            label: "PERSP".into(),
            status: "00000000",
            parameters: "410,2,1.5,0,0,1,0,0,0,0,0,10,0,1,0,5,-2,2,-1,1,3,-5,5;".into(),
        },
    ])
}

fn view_visibility_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW1".into(),
            status: "00000000",
            parameters: "410,1,1,0,0,0,0,0,0,1,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 3,
            label: "VISIBLE".into(),
            status: "00000000",
            parameters: "402,1,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW2".into(),
            status: "00000000",
            parameters: "410,2,1,0,0,0,0,0,0,1,7,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 4,
            label: "DISPLAY".into(),
            status: "00000000",
            parameters: "402,1,0,5,1,0,2,3;".into(),
        },
    ])
}

fn segmented_view_visibility_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW".into(),
            status: "00000000",
            parameters: "410,1,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 19,
            label: "SEGMENTS".into(),
            status: "00000000",
            parameters: "402,2,1,0.5,0,,,1,1,1.0,1,2,3,4;".into(),
        },
    ])
}

fn drawing_with_properties_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW".into(),
            status: "00020000",
            parameters: "410,1,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "NOTELOC".into(),
            status: "00010100",
            parameters: "116,5,6,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 16,
            label: "SIZE".into(),
            status: "00010000",
            parameters: "406,2,210,297;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 17,
            label: "UNITS".into(),
            status: "00010000",
            parameters: "406,2,2,2HMM;".into(),
        },
        OwnedTestEntity {
            entity_type: 406,
            form: 15,
            label: "NAME".into(),
            status: "00010000",
            parameters: "406,1,7HDETAIL1;".into(),
        },
        OwnedTestEntity {
            entity_type: 404,
            form: 1,
            label: "DRAWING".into(),
            status: "00000000",
            parameters: "404,1,1,10,20,0.5,1,3,0,3,5,7,9;".into(),
        },
    ])
}

fn text_annotation_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "NOTE".into(),
            status: "00000100",
            parameters: "212,2,5,20,4,1,1.5707963267948966,0,0,0,1,2,0,5HALPHA,3,12,3,18,1.5707963267948966,0.25,1,1,4,5,0,3HBET;".into(),
        },
        OwnedTestEntity {
            entity_type: 213,
            form: 0,
            label: "NEWNOTE".into(),
            status: "00000100",
            parameters: "213,40,20,2,0,20,0,0,0,18,0,-5,1,0,2,3,-0.5,0,18,0,4HTUNL,4,12,3,1,1.5707963267948966,0,0,0,2,18,0,4HTOL!;".into(),
        },
    ])
}

fn leader_forms_file() -> Vec<u8> {
    let entities = (1..=12)
        .map(|form| {
            let (height, width) = match form {
                4 => (0, 0),
                5 | 6 | 12 => (2, 2),
                _ => (2, 1),
            };
            OwnedTestEntity {
                entity_type: 214,
                form,
                label: format!("LEAD{form}"),
                status: "00000100",
                parameters: format!("214,2,{height},{width},3,0,0,5,0,5,4;"),
            }
        })
        .collect::<Vec<_>>();
    owned_test_file(&entities)
}

fn dimension_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "DIMNOTE".into(),
            status: "00010100",
            parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
        },
        OwnedTestEntity {
            entity_type: 214,
            form: 1,
            label: "ARROW".into(),
            status: "00010100",
            parameters: "214,3,2,1,0,0,0,2,0,2,2,4,2;".into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 40,
            label: "WITNESS".into(),
            status: "00010100",
            parameters: "106,1,3,0,0,0,1,0,1,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 100,
            form: 0,
            label: "ENCLOSE".into(),
            status: "00010100",
            parameters: "100,0,0,0,1,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 214,
            form: 4,
            label: "NOARROW".into(),
            status: "00010100",
            parameters: "214,1,0,0,0,0,0,2,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 216,
            form: 0,
            label: "LINEAR0".into(),
            status: "00000100",
            parameters: "216,1,3,3,5,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 216,
            form: 1,
            label: "LINEAR1".into(),
            status: "00000100",
            parameters: "216,1,3,3,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 216,
            form: 2,
            label: "LINEAR2".into(),
            status: "00000100",
            parameters: "216,1,3,3,5,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 218,
            form: 0,
            label: "ORD0".into(),
            status: "00000100",
            parameters: "218,1,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 218,
            form: 1,
            label: "ORD1".into(),
            status: "00000100",
            parameters: "218,1,5,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 220,
            form: 0,
            label: "POINTDIM".into(),
            status: "00000100",
            parameters: "220,1,3,7;".into(),
        },
        OwnedTestEntity {
            entity_type: 222,
            form: 0,
            label: "RADIUS0".into(),
            status: "00000100",
            parameters: "222,1,3,10,20;".into(),
        },
        OwnedTestEntity {
            entity_type: 222,
            form: 1,
            label: "RADIUS1".into(),
            status: "00000100",
            parameters: "222,1,3,10,20,9;".into(),
        },
    ])
}

fn legacy_dimension_and_label_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "NOTE".into(),
            status: "00010100",
            parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;".into(),
        },
        OwnedTestEntity {
            entity_type: 214,
            form: 1,
            label: "LEADER1".into(),
            status: "00010100",
            parameters: "214,1,2,1,0,0,0,2,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 214,
            form: 1,
            label: "LEADER2".into(),
            status: "00010100",
            parameters: "214,1,2,1,0,1,0,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 40,
            label: "WITNESS".into(),
            status: "00010100",
            parameters: "106,1,3,0,0,0,1,0,1,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "CURVE1".into(),
            status: "00010100",
            parameters: "110,0,0,0,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 100,
            form: 0,
            label: "CURVE2".into(),
            status: "00010100",
            parameters: "100,0,0,0,1,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 202,
            form: 0,
            label: "ANGULAR".into(),
            status: "00000100",
            parameters: "202,1,7,0,0,0,2,3,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 204,
            form: 0,
            label: "CURVEDIM".into(),
            status: "00000100",
            parameters: "204,1,9,11,3,5,7,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 206,
            form: 0,
            label: "DIAMETER".into(),
            status: "00000100",
            parameters: "206,1,3,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 208,
            form: 0,
            label: "FLAGNOTE".into(),
            status: "00000100",
            parameters: "208,0,0,0,0,1,2,3,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 210,
            form: 0,
            label: "LABEL".into(),
            status: "00000100",
            parameters: "210,1,1,3;".into(),
        },
    ])
}

fn symbol_and_sectioned_area_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "SYMNOTE".into(),
            status: "00010100",
            parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HS;".into(),
        },
        OwnedTestEntity {
            entity_type: 100,
            form: 0,
            label: "SYMGEOM".into(),
            status: "00010100",
            parameters: "100,0,0,0,1,0,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 214,
            form: 2,
            label: "SYMLEAD".into(),
            status: "00010100",
            parameters: "214,1,2,1,0,0,0,2,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 228,
            form: 0,
            label: "SYMBOL".into(),
            status: "00000100",
            parameters: "228,1,1,3,1,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 100,
            form: 0,
            label: "BOUNDARY".into(),
            status: "00000000",
            parameters: "100,0,0,0,5,0,5,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 100,
            form: 0,
            label: "ISLAND".into(),
            status: "00000000",
            parameters: "100,0,0,0,1,0,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 230,
            form: 0,
            label: "SECTION".into(),
            status: "00000100",
            parameters: "230,9,2,0,0,0,1,0.7853981633974483,1,11;".into(),
        },
    ])
}

fn associativity_definition_file() -> Vec<u8> {
    owned_test_file(&[OwnedTestEntity {
        entity_type: 302,
        form: 5001,
        label: "ASSOCDEF".into(),
        status: "00000200",
        parameters: "302,2,1,1,2,1,2,2,2,1,3;".into(),
    }])
}

fn bounded_associativity_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW".into(),
            status: "00000000",
            parameters: "410,1,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 214,
            form: 1,
            label: "LABELARR".into(),
            status: "00010100",
            parameters: "214,1,2,1,0,0,0,2,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "LABELED".into(),
            status: "00000000",
            parameters: "116,1,2,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 5,
            label: "LABELDSP".into(),
            status: "00000200",
            parameters: "402,1,1,1,2,3,3,0,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "PARENT".into(),
            status: "00000000",
            parameters: "116,0,0,0,0,1,13,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "CHILD".into(),
            status: "00000000",
            parameters: "116,1,0,0,0,1,13,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 9,
            label: "PARENTCH".into(),
            status: "00000200",
            parameters: "402,1,1,9,11;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 12,
            label: "EXTINDEX".into(),
            status: "00000200",
            parameters: "402,1,4HNAME,9;".into(),
        },
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "DIMNOTE".into(),
            status: "00010100",
            parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HD;".into(),
        },
        OwnedTestEntity {
            entity_type: 214,
            form: 1,
            label: "DIMARR".into(),
            status: "00010100",
            parameters: "214,1,2,1,0,0,0,2,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 216,
            form: 0,
            label: "DIMENS".into(),
            status: "00000100",
            parameters: "216,17,19,19,0,0,1,23,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 13,
            label: "DIMGEOM".into(),
            status: "00000200",
            parameters: "402,1,1,21,9;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 16,
            label: "PLANAR".into(),
            status: "00000200",
            parameters: "402,1,2,0,9,11;".into(),
        },
    ])
}

fn view_list_associativity_file(back_pointers: bool) -> Vec<u8> {
    let suffix = if back_pointers { ",1,3,0" } else { "" };
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW".into(),
            status: "00000000",
            parameters: format!("410,1,1,0,0,0,0,0,0{suffix};"),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 6,
            label: "VIEWLIST".into(),
            status: "00000200",
            parameters: "402,1,1,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "VISIBLE".into(),
            status: "00000000",
            parameters: format!("116,1,2,3,0{suffix};"),
        },
    ])
}

fn flow_associativity_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 132,
            form: 0,
            label: "SIGNALPT".into(),
            status: "00000400",
            parameters: "132,0,0,0,0,101,1,2HP1,0,3HPIN,0,1,1,0,0,1,7,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "SIGNAL".into(),
            status: "00000000",
            parameters: "110,0,0,0,1,0,0,1,7,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "FLOWNAME".into(),
            status: "00000100",
            parameters: "212,1,4,4,1,1,1.5707963267948966,0,0,0,0,0,0,4HFLOW;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 18,
            label: "FLOW".into(),
            status: "00000200",
            parameters: "402,2,0,1,1,1,1,1,1,2,1,3,4HFLOW,5,9;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 18,
            label: "FLOWTAIL".into(),
            status: "00000200",
            parameters: "402,2,0,0,0,1,0,0,1,2,4HTAIL,1,7,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 132,
            form: 0,
            label: "PIPEPT".into(),
            status: "00000400",
            parameters: "132,0,0,0,0,101,1,2HP2,0,4HPIPE,0,2,2,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "PIPE".into(),
            status: "00000000",
            parameters: "110,0,0,0,2,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 20,
            label: "PIPEFLOW".into(),
            status: "00000200",
            parameters: "402,1,0,1,1,1,0,1,2,11,13,4HPIPE,17;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 20,
            label: "PIPETAIL".into(),
            status: "00000200",
            parameters: "402,1,0,0,0,1,0,0,2,4HTAIL;".into(),
        },
    ])
}

fn recalculable_dimension_associativity_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 212,
            form: 0,
            label: "DIMNOTE".into(),
            status: "00010100",
            parameters: "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HD;".into(),
        },
        OwnedTestEntity {
            entity_type: 214,
            form: 2,
            label: "ARROW1".into(),
            status: "00010100",
            parameters: "214,1,2,1,0,0,0,2,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 214,
            form: 2,
            label: "ARROW2".into(),
            status: "00010100",
            parameters: "214,1,2,1,0,4,0,2,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "GEOM1".into(),
            status: "00000000",
            parameters: "110,0,0,0,0,4,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "GEOM2".into(),
            status: "00000000",
            parameters: "110,4,0,0,4,4,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 216,
            form: 0,
            label: "DIMENS".into(),
            status: "00000100",
            parameters: "216,1,3,5,0,0,1,13,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 21,
            label: "RECALCD".into(),
            status: "00010200",
            parameters: "402,1,2,11,4,0,7,0,0,0,0,9,1,4,0,0;".into(),
        },
    ])
}

fn text_display_template_forms_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 312,
            form: 0,
            label: "ABSTEXT".into(),
            status: "00000200",
            parameters: "312,4,2,1,1.5707963267948966,0,0,0,10,20,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 312,
            form: 1,
            label: "INCTEXT".into(),
            status: "00000200",
            parameters: "312,3,1,18,1.5707963267948966,0.25,1,1,2,-1,0;".into(),
        },
    ])
}

fn text_font_definition_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 310,
            form: 0,
            label: "BASEFONT".into(),
            status: "00000200",
            parameters: "310,101,4HBASE,,10,2,65,8,0,3,,0,0,0,4,10,0,8,0,66,8,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 310,
            form: 0,
            label: "MODFONT".into(),
            status: "00000200",
            parameters: "310,102,3HMOD,-1,10,1,67,8,0,2,1,0,0,,8,10;".into(),
        },
        OwnedTestEntity {
            entity_type: 312,
            form: 0,
            label: "FONTUSE".into(),
            status: "00000200",
            parameters: "312,4,2,-3,1.5707963267948966,0,0,0,0,0,0;".into(),
        },
    ])
}

fn units_data_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "MEASURED".into(),
            status: "00000000",
            parameters: "110,0,0,0,1,0,0,0,1,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 316,
            form: 0,
            label: "UNITS".into(),
            status: "00000200",
            parameters: "316,3,6HLENGTH,1HM,1000,4HTIME,1HS,1,5HPLANE,1HD,0.017453292519943295;"
                .into(),
        },
    ])
}

fn nested_subfigure_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "MEMBER".into(),
            status: "00010000",
            parameters: "110,0,0,0,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 308,
            form: 0,
            label: "CHILD".into(),
            status: "00000200",
            parameters: "308,0,5HCHILD,1,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 408,
            form: 0,
            label: "CHILDINS".into(),
            status: "00000000",
            parameters: "408,3,1,2,3,0.5;".into(),
        },
        OwnedTestEntity {
            entity_type: 308,
            form: 0,
            label: "PARENT".into(),
            status: "00000200",
            parameters: "308,1,6HPARENT,1,5;".into(),
        },
        OwnedTestEntity {
            entity_type: 408,
            form: 0,
            label: "PARENTIN".into(),
            status: "00000000",
            parameters: "408,7,10,20,30,2;".into(),
        },
    ])
}

fn occurrence_limit_file() -> Vec<u8> {
    let mut entities = vec![OwnedTestEntity {
        entity_type: 308,
        form: 0,
        label: "EMPTYDEF".into(),
        status: "00000200",
        parameters: "308,0,8HEMPTYDEF,0;".into(),
    }];
    entities.extend((0..101).map(|_| OwnedTestEntity {
        entity_type: 408,
        form: 0,
        label: "INSTANCE".into(),
        status: "00000000",
        parameters: "408,1,0,0,0,1;".into(),
    }));
    owned_test_file(&entities)
}

fn invalid_subfigure_depth_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 308,
            form: 0,
            label: "CHILD".into(),
            status: "00000200",
            parameters: "308,0,5HCHILD,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 408,
            form: 0,
            label: "INSTANCE".into(),
            status: "00000000",
            parameters: "408,1,0,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 308,
            form: 0,
            label: "PARENT".into(),
            status: "00000200",
            parameters: "308,0,6HPARENT,1,3;".into(),
        },
    ])
}

fn network_subfigure_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 320,
            form: 0,
            label: "NETWORK".into(),
            status: "00000200",
            parameters: "320,0,3HNET,0,1,2HR1,0,2,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 420,
            form: 0,
            label: "NETINST".into(),
            status: "00000000",
            parameters: "420,1,1,2,3,2,,,1,2HU1,0,2,0,0;".into(),
        },
    ])
}

fn connected_network_subfigure_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 132,
            form: 0,
            label: "DEFPIN".into(),
            status: "00000400",
            parameters: "132,0,0,0,0,101,1,2HP1,0,3HPIN,0,1,1,0,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 320,
            form: 0,
            label: "NETWORK".into(),
            status: "00000200",
            parameters: "320,0,3HNET,0,1,2HR1,0,1,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 132,
            form: 0,
            label: "INSTPIN".into(),
            status: "00000400",
            parameters: "132,1,2,3,0,101,1,2HP1,0,3HPIN,0,2,1,0,7;".into(),
        },
        OwnedTestEntity {
            entity_type: 420,
            form: 0,
            label: "NETINST".into(),
            status: "00000000",
            parameters: "420,3,10,20,30,1,,,1,2HU1,0,1,5;".into(),
        },
    ])
}

fn explicit_multi_pcurve_loop_file() -> Vec<u8> {
    explicit_multi_pcurve_loop_file_with_first_pcurve(
        "126,1,1,1,0,1,0,0,0,1,1,1,1,0,0,0,0.5,0,0,0,1,0,0,1;",
    )
}

fn explicit_multi_pcurve_loop_file_with_first_pcurve(first_pcurve: &str) -> Vec<u8> {
    explicit_multi_pcurve_loop_file_with_carriers(first_pcurve, "110,0,0,0,1,0,0;")
}

fn explicit_multi_pcurve_loop_file_with_first_edge(first_edge: &str) -> Vec<u8> {
    explicit_multi_pcurve_loop_file_with_carriers(
        "126,1,1,1,0,1,0,0,0,1,1,1,1,0,0,0,0.5,0,0,0,1,0,0,1;",
        first_edge,
    )
}

fn explicit_multi_pcurve_loop_file_with_carriers(first_pcurve: &str, first_edge: &str) -> Vec<u8> {
    let mut entities = vec![
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "LOCATION".into(),
            status: "00010000",
            parameters: "116,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 123,
            form: 0,
            label: "NORMAL".into(),
            status: "00010000",
            parameters: "123,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 190,
            form: 0,
            label: "SURFACE".into(),
            status: "00010000",
            parameters: "190,1,3;".into(),
        },
    ];
    for (index, parameters) in [
        first_edge,
        "110,1,0,0,1,1,0;",
        "110,1,1,0,0,1,0;",
        "110,0,1,0,0,0,0;",
    ]
    .into_iter()
    .enumerate()
    {
        entities.push(OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: format!("EDGE{}", index + 1),
            status: "00010000",
            parameters: parameters.into(),
        });
    }
    entities.extend([
        OwnedTestEntity {
            entity_type: 502,
            form: 1,
            label: "VERTICES".into(),
            status: "00010000",
            parameters: "502,4,0,0,0,1,0,0,1,1,0,0,1,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 504,
            form: 1,
            label: "EDGES".into(),
            status: "00010001",
            parameters: "504,4,7,15,1,15,2,9,15,2,15,3,11,15,3,15,4,13,15,4,15,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 126,
            form: 1,
            label: "PCURVE1".into(),
            status: "00010500",
            parameters: first_pcurve.into(),
        },
        OwnedTestEntity {
            entity_type: 126,
            form: 1,
            label: "PCURVE2".into(),
            status: "00010500",
            parameters: "126,1,1,1,0,1,0,0,0,1,1,1,1,0.5,0,0,1,0,0,0,1,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 508,
            form: 1,
            label: "LOOP".into(),
            status: "00010000",
            parameters: "508,5,0,17,1,1,2,1,19,0,21,1,15,2,0,0,0,17,2,1,0,0,17,3,1,0,0,17,4,1,0;"
                .into(),
        },
        OwnedTestEntity {
            entity_type: 510,
            form: 1,
            label: "FACE".into(),
            status: "00010000",
            parameters: "510,5,1,1,23;".into(),
        },
        OwnedTestEntity {
            entity_type: 514,
            form: 2,
            label: "SHELL".into(),
            status: "00000000",
            parameters: "514,1,25,1;".into(),
        },
    ]);
    owned_test_file(&entities)
}

fn explicit_cylinder_seam_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "ORIGIN".into(),
            status: "00010000",
            parameters: "116,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 123,
            form: 0,
            label: "AXIS".into(),
            status: "00010000",
            parameters: "123,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 192,
            form: 0,
            label: "CYLINDER".into(),
            status: "00010000",
            parameters: "192,1,3,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "SEAMEDGE".into(),
            status: "00010000",
            parameters: "110,1,0,0,1,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 502,
            form: 1,
            label: "VERTICES".into(),
            status: "00010000",
            parameters: "502,2,1,0,0,1,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 504,
            form: 1,
            label: "EDGES".into(),
            status: "00010001",
            parameters: "504,1,7,9,1,9,2;".into(),
        },
        OwnedTestEntity {
            entity_type: 126,
            form: 1,
            label: "SEAMUV0".into(),
            status: "00010500",
            parameters: "126,1,1,1,0,1,0,0,0,1,1,1,1,0,0,0,0,1,0,0,1,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 126,
            form: 1,
            label: "SEAMUV1".into(),
            status: "00010500",
            parameters: format!(
                "126,1,1,1,0,1,0,0,0,1,1,1,1,{},{},0,{},0,0,0,1,0,0,1;",
                std::f64::consts::TAU,
                1,
                std::f64::consts::TAU
            ),
        },
        OwnedTestEntity {
            entity_type: 508,
            form: 1,
            label: "SEAMLOOP".into(),
            status: "00010000",
            parameters: "508,2,0,11,1,1,1,1,13,0,11,1,0,1,1,15;".into(),
        },
        OwnedTestEntity {
            entity_type: 510,
            form: 1,
            label: "SEAMFACE".into(),
            status: "00010000",
            parameters: "510,5,1,1,17;".into(),
        },
        OwnedTestEntity {
            entity_type: 514,
            form: 2,
            label: "SEAMSHEL".into(),
            status: "00000000",
            parameters: "514,1,19,1;".into(),
        },
    ])
}

fn multi_pcurve_boundary_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 108,
            form: 0,
            label: "PLANE".into(),
            status: "00010000",
            parameters: "108,0,0,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 63,
            label: "MODEL".into(),
            status: "00010000",
            parameters: "106,1,5,0,0,0,1,0,1,1,0,1,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 126,
            form: 1,
            label: "PCURVE1".into(),
            status: "00010500",
            parameters: "126,1,1,1,0,1,0,0,0,1,1,1,1,0,0,0,1,1,0,0,1,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 126,
            form: 1,
            label: "PCURVE2".into(),
            status: "00010500",
            parameters: "126,1,1,1,0,1,0,0,0,1,1,1,1,1,1,0,0,0,0,0,1,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 141,
            form: 0,
            label: "BOUNDARY".into(),
            status: "00010000",
            parameters: "141,1,3,1,1,3,1,2,5,7;".into(),
        },
        OwnedTestEntity {
            entity_type: 143,
            form: 0,
            label: "BOUNDED".into(),
            status: "00000000",
            parameters: "143,1,1,1,9;".into(),
        },
    ])
}

fn trimmed_plane_with_inner_loop_file() -> Vec<u8> {
    let outer = "106,1,5,0,0,0,1,0,1,1,0,1,0,0;";
    trimmed_plane_with_inner_loop_and_outer_pcurve(outer)
}

fn trimmed_plane_with_inner_loop_and_outer_pcurve(outer_pcurve: &str) -> Vec<u8> {
    let outer = "106,1,5,0,0,0,1,0,1,1,0,1,0,0;";
    let inner = "106,1,5,0,0.25,0.25,0.75,0.25,0.75,0.75,0.25,0.75,0.25,0.25;";
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 108,
            form: 0,
            label: "PLANE".into(),
            status: "00010000",
            parameters: "108,0,0,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 63,
            label: "OUTMODEL".into(),
            status: "00010000",
            parameters: outer.into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 63,
            label: "OUTPCURV".into(),
            status: "00010500",
            parameters: outer_pcurve.into(),
        },
        OwnedTestEntity {
            entity_type: 142,
            form: 0,
            label: "OUTBOUND".into(),
            status: "00010000",
            parameters: "142,0,1,5,3,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 63,
            label: "INMODEL".into(),
            status: "00010000",
            parameters: inner.into(),
        },
        OwnedTestEntity {
            entity_type: 106,
            form: 63,
            label: "INPCURVE".into(),
            status: "00010500",
            parameters: inner.into(),
        },
        OwnedTestEntity {
            entity_type: 142,
            form: 0,
            label: "INBOUND".into(),
            status: "00010000",
            parameters: "142,0,1,11,9,3;".into(),
        },
        OwnedTestEntity {
            entity_type: 144,
            form: 0,
            label: "ANNULUS".into(),
            status: "00000000",
            parameters: "144,1,1,1,7,13;".into(),
        },
    ])
}

fn parameter_domain_trimmed_surface_file() -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 128,
            form: 0,
            label: "SURFACE".into(),
            status: "00010000",
            parameters:
                "128,1,1,1,1,0,0,1,0,0,0,0,1,1,0,0,1,1,1,1,1,1,0,0,0,1,0,0,0,1,0,1,1,0,0,1,0,1;"
                    .into(),
        },
        OwnedTestEntity {
            entity_type: 144,
            form: 0,
            label: "DOMAIN".into(),
            status: "00000000",
            parameters: "144,1,0,0,0;".into(),
        },
    ])
}

#[test]
fn decode_classifies_explicit_outer_and_inner_trimmed_surface_loops() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_plane_with_inner_loop_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let face = result
        .ir
        .model
        .faces
        .iter()
        .find(|face| face.id.0 == "iges:model:face#D15")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report.losses));
    assert_eq!(face.loops.len(), 2);
    let roles = face
        .loops
        .iter()
        .map(|id| {
            result
                .ir
                .model
                .loops
                .iter()
                .find(|loop_| loop_.id == *id)
                .unwrap()
                .boundary_role
        })
        .collect::<Vec<_>>();
    assert_eq!(
        roles,
        vec![
            cadmpeg_ir::topology::LoopBoundaryRole::Outer,
            cadmpeg_ir::topology::LoopBoundaryRole::Inner,
        ]
    );
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_preserves_parameter_domain_as_implicit_outer_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(parameter_domain_trimmed_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let face = result
        .ir
        .model
        .faces
        .iter()
        .find(|face| face.id.0 == "iges:model:face#D3")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report.losses));
    assert!(face.loops.is_empty());
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_rejects_disagreeing_curve_on_surface_carriers() {
    let shifted_outer = "106,1,5,0,0.1,0,1.1,0,1.1,1,0.1,1,0.1,0;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_plane_with_inner_loop_and_outer_pcurve(
                shifted_outer,
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result
        .ir
        .model
        .faces
        .iter()
        .all(|face| face.id.0 != "iges:model:face#D15"));
    assert!(result.report.losses.iter().any(|loss| loss
        .message
        .contains("carriers disagree beyond the minimum resolution")));
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_preserves_ordered_type_141_pcurve_collections() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(multi_pcurve_boundary_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let coedge = result
        .ir
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id.0 == "iges:model:coedge#D11:0:0")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report.losses));
    assert_eq!(coedge.pcurves.len(), 2);
    assert!(coedge.pcurves[0].pcurve.0.ends_with(":0:0:0"));
    assert!(coedge.pcurves[1].pcurve.0.ends_with(":0:0:1"));
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_preserves_two_uses_and_periodic_images_of_a_cylinder_seam() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(explicit_cylinder_seam_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let loop_ = result
        .ir
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id.0 == "iges:model:loop#D21:D17")
        .unwrap();
    assert_eq!(loop_.coedges.len(), 2);
    let coedges = loop_
        .coedges
        .iter()
        .map(|id| {
            result
                .ir
                .model
                .coedges
                .iter()
                .find(|coedge| coedge.id == *id)
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(coedges[0].edge, coedges[1].edge);
    assert_ne!(coedges[0].sense, coedges[1].sense);
    assert_eq!(coedges[0].radial_next, coedges[1].id);
    assert_eq!(coedges[1].radial_next, coedges[0].id);
    let seam_u = coedges
        .iter()
        .map(|coedge| {
            let pcurve = result
                .ir
                .model
                .pcurves
                .iter()
                .find(|pcurve| pcurve.id == coedge.pcurves[0].pcurve)
                .unwrap();
            cadmpeg_ir::eval::pcurve_uv(&pcurve.geometry, 0.0)
                .unwrap()
                .u
        })
        .collect::<Vec<_>>();
    assert!((seam_u[0] - 0.0).abs() < 1.0e-12);
    assert!((seam_u[1] - std::f64::consts::TAU).abs() < 1.0e-12);
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_preserves_ordered_loop_pcurve_collection_and_isoparametric_flags() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(explicit_multi_pcurve_loop_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let coedge = result
        .ir
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id.0 == "iges:model:coedge#D27:D23:0")
        .unwrap();
    assert_eq!(coedge.pcurves.len(), 2);
    assert_eq!(coedge.pcurves[0].isoparametric, Some(true));
    assert_eq!(coedge.pcurves[1].isoparametric, Some(false));
    assert!(coedge.pcurves[0].pcurve.0.ends_with(":0:0"));
    assert!(coedge.pcurves[1].pcurve.0.ends_with(":0:1"));
    let loop_ = result
        .ir
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id.0 == "iges:model:loop#D27:D23")
        .unwrap();
    assert_eq!(loop_.vertex_uses.len(), 1);
    assert_eq!(loop_.vertex_uses[0].vertex.0, "iges:model:vertex#D27:D15:2");
    assert_eq!(loop_.vertex_uses[0].after.as_ref(), Some(&coedge.id));
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_rejects_disagreeing_explicit_loop_pcurves() {
    let shifted = "126,1,1,1,0,1,0,0,0,1,1,1,1,0.1,0,0,0.5,0,0,0,1,0,0,1;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(explicit_multi_pcurve_loop_file_with_first_pcurve(shifted)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result
        .ir
        .model
        .bodies
        .iter()
        .all(|body| body.id.0 != "iges:model:body#D27"));
    assert!(result.report.losses.iter().any(|loss| loss
        .message
        .contains("loop edge-use pcurves disagree with the edge vertices")));
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_rejects_explicit_edges_that_miss_their_vertices() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(explicit_multi_pcurve_loop_file_with_first_edge(
                "110,0,0,0,1.1,0,0;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result
        .ir
        .model
        .bodies
        .iter()
        .all(|body| body.id.0 != "iges:model:body#D27"));
    assert!(result.report.losses.iter().any(|loss| loss
        .message
        .contains("edge curve endpoints disagree with the vertex-list points")));
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_a_vertex_only_pole_loop() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(explicit_vertex_loop_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let loop_ = result
        .ir
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id.0 == "iges:model:loop#D11:D7")
        .unwrap_or_else(|| {
            panic!(
                "loops={:#?} losses={:#?}",
                result.ir.model.loops, result.report.losses
            )
        });
    assert!(loop_.coedges.is_empty());
    assert_eq!(loop_.vertex_uses.len(), 1);
    assert_eq!(loop_.vertex_uses[0].vertex.0, "iges:model:vertex#D11:D5:1");
    assert!(loop_.vertex_uses[0].after.is_none());
    assert!(loop_.vertex_uses[0].pcurves.is_empty());
    assert_eq!(
        loop_.boundary_role,
        cadmpeg_ir::topology::LoopBoundaryRole::Outer
    );
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_preserves_a_face_with_no_explicit_outer_loop() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(explicit_vertex_loop_file_with_outer_flag(false)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let loop_ = result
        .ir
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id.0 == "iges:model:loop#D11:D7")
        .unwrap();
    assert_eq!(
        loop_.boundary_role,
        cadmpeg_ir::topology::LoopBoundaryRole::Inner
    );
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_applies_standard_body_color_and_face_color_override() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(colored_explicit_vertex_loop_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let body = result
        .ir
        .model
        .bodies
        .iter()
        .find(|body| body.id.0 == "iges:model:body#D11")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report.losses));
    assert_eq!(
        body.color,
        Some(cadmpeg_ir::topology::Color {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        })
    );
    assert_eq!(body.visible, Some(true));
    let face = result
        .ir
        .model
        .faces
        .iter()
        .find(|face| face.id.0 == "iges:model:face#D11:D9")
        .unwrap();
    assert_eq!(
        face.color,
        Some(cadmpeg_ir::topology::Color {
            r: 0.2,
            g: 0.4,
            b: 0.6,
            a: 1.0,
        })
    );
    assert!(result
        .ir
        .model
        .appearances
        .iter()
        .any(|appearance| appearance.id.0 == "iges:appearance:color#D13"
            && appearance.name.as_deref() == Some("custom")));
    assert_eq!(result.ir.model.appearance_bindings.len(), 2);
    let native = result.ir.native.namespace("iges").unwrap();
    assert_eq!(native.version, 2);
    assert_eq!(native.arenas["colors"].len(), 1);
    assert_eq!(native.arenas["colors"][0].id, "iges:presentation:color#D13");
    assert_eq!(native.arenas["colors"][0].fields["red_percent"], 20.0);
    assert_eq!(native.arenas["display_attributes"].len(), 7);
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_types_template_and_visible_blank_line_fonts() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(line_font_definitions_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir.native.namespace("iges").unwrap();
    let line_fonts = &native.arenas["line_fonts"];
    assert_eq!(line_fonts.len(), 2);
    assert_eq!(line_fonts[0].id, "iges:presentation:line-font#D3");
    assert_eq!(line_fonts[0].fields["kind"], "template");
    assert_eq!(line_fonts[0].fields["tangent_oriented"], true);
    assert_eq!(line_fonts[0].fields["template"], "iges:entity:directory#1");
    assert_eq!(line_fonts[1].fields["kind"], "visible_blank_pattern");
    assert_eq!(line_fonts[1].fields["segment_count"], 5);
    assert_eq!(
        line_fonts[1].fields["hexadecimal_pattern"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![49, 54]
    );
    let line_display = native.arenas["display_attributes"]
        .iter()
        .find(|record| record.id == "iges:presentation:display-attributes#D7")
        .unwrap();
    assert_eq!(line_display.fields["line_font_number"], -5);
    assert_eq!(
        line_display.fields["line_font_definition"],
        "iges:entity:directory#5"
    );
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_types_definition_levels_and_directory_level_links() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(definition_levels_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir.native.namespace("iges").unwrap();
    let levels = &native.arenas["definition_levels"];
    assert_eq!(levels.len(), 1);
    assert_eq!(levels[0].id, "iges:presentation:definition-levels#D1");
    assert_eq!(levels[0].fields["declared_count"], 3);
    assert_eq!(
        levels[0].fields["levels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![2, 7, 11]
    );
    let line = native.arenas["display_attributes"]
        .iter()
        .find(|record| record.id == "iges:presentation:display-attributes#D3")
        .unwrap();
    assert_eq!(line.fields["level_number"], -1);
    assert_eq!(
        line.fields["level_definition"],
        "iges:presentation:definition-levels#D1"
    );
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_resolves_directory_line_weight_to_millimetres() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(weighted_line_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let display = &result.ir.native.namespace("iges").unwrap().arenas["display_attributes"][0];
    assert_eq!(display.fields["line_weight_number"], 1);
    assert_eq!(display.fields["line_weight_mm"], 1.0);
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_types_all_csg_primitive_solids_and_defaults() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(primitive_solids_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let solids = &result.ir.native.namespace("iges").unwrap().arenas["primitive_solids"];
    assert_eq!(solids.len(), 8);
    let block = solids
        .iter()
        .find(|solid| solid.id == "iges:solid:primitive#D1")
        .unwrap();
    assert_eq!(block.fields["kind"], "block");
    assert_eq!(block.fields["dimensions"]["x_length"], 2.0);
    assert_eq!(block.fields["origin"][0], 1.0);
    let default_block = solids
        .iter()
        .find(|solid| solid.id == "iges:solid:primitive#D3")
        .unwrap();
    assert!(default_block.fields["origin"][0].is_null());
    assert_eq!(
        solids
            .iter()
            .map(|solid| solid.fields["kind"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([
            "block",
            "ellipsoid",
            "right_angular_wedge",
            "right_circular_cone_frustum",
            "right_circular_cylinder",
            "sphere",
            "torus",
        ])
    );
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_rejects_invalid_csg_primitive_dimensions_semantically() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 160,
        form: 0,
        label: "TORUS".into(),
        status: "00000000",
        parameters: "160,1,2,0,0,0,0,0,1;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        result.ir.native.namespace("iges").unwrap().arenas["primitive_solids"].len(),
        1
    );
    assert!(result.report.losses.iter().any(|loss| loss
        .message
        .contains("primitive dimension invariant is violated")));
    assert!(!result.report.geometry_transferred);
}

#[test]
fn decode_types_swept_solids_and_balanced_boolean_postfix() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(procedural_and_boolean_solids_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir.native.namespace("iges").unwrap();
    let procedural = &native.arenas["procedural_solids"];
    assert_eq!(procedural.len(), 3);
    let open_revolution = procedural
        .iter()
        .find(|solid| solid.id == "iges:solid:procedural#D5")
        .unwrap();
    assert_eq!(open_revolution.fields["kind"], "revolution");
    assert_eq!(open_revolution.fields["form"], 0);
    assert_eq!(open_revolution.fields["amount"], 0.5);
    let closed_revolution = procedural
        .iter()
        .find(|solid| solid.id == "iges:solid:procedural#D7")
        .unwrap();
    assert_eq!(closed_revolution.fields["form"], 1);
    let extrusion = procedural
        .iter()
        .find(|solid| solid.id == "iges:solid:procedural#D9")
        .unwrap();
    assert_eq!(extrusion.fields["kind"], "linear_extrusion");
    let trees = &native.arenas["boolean_trees"];
    assert_eq!(trees.len(), 1);
    assert_eq!(trees[0].fields["declared_length"], 3);
    assert_eq!(trees[0].fields["terms"].as_array().unwrap().len(), 3);
    let selected = &native.arenas["selected_components"];
    assert_eq!(selected.len(), 1);
    assert_eq!(
        selected[0].fields["boolean_tree"],
        "iges:solid:boolean-tree#D15"
    );
    assert_eq!(selected[0].fields["selection_point"][0], 1.0);
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_types_form_one_boolean_tree_with_brep_operand() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_with_boolean_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let trees = &result.ir.native.namespace("iges").unwrap().arenas["boolean_trees"];
    let tree = trees
        .iter()
        .find(|tree| tree.id == "iges:solid:boolean-tree#D59")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report.losses));
    assert_eq!(tree.fields["form"], 1);
    assert_eq!(
        tree.fields["terms"][0]["entity"],
        "iges:entity:directory#55"
    );
    let assembly = result.ir.native.namespace("iges").unwrap().arenas["solid_assemblies"]
        .iter()
        .find(|assembly| assembly.id == "iges:product:solid-assembly#D61")
        .unwrap();
    assert_eq!(assembly.fields["form"], 1);
    assert_eq!(
        assembly.fields["items"][0]["item"],
        "iges:entity:directory#55"
    );
    let instance = result.ir.native.namespace("iges").unwrap().arenas["solid_instances"]
        .iter()
        .find(|instance| instance.id == "iges:product:solid-instance#D63")
        .unwrap();
    assert_eq!(instance.fields["form"], 1);
    assert_eq!(instance.fields["solid"], "iges:entity:directory#55");
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_preserves_solid_definition_and_instance_identities() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(solid_instance_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let instances = &result.ir.native.namespace("iges").unwrap().arenas["solid_instances"];
    assert_eq!(instances.len(), 1);
    assert_eq!(instances[0].id, "iges:product:solid-instance#D3");
    assert_eq!(instances[0].fields["solid"], "iges:entity:directory#1");
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_preserves_rectangular_and_circular_pattern_order() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(patterned_instance_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir.native.namespace("iges").unwrap();
    let rectangular = &native.arenas["rectangular_arrays"][0];
    assert_eq!(rectangular.fields["base"], "iges:entity:directory#1");
    assert_eq!(rectangular.fields["columns"], 2);
    assert_eq!(rectangular.fields["rows"], 3);
    assert_eq!(rectangular.fields["positions"][0], 2);
    let circular = &native.arenas["circular_arrays"][0];
    assert_eq!(circular.fields["base"], "iges:entity:directory#3");
    assert_eq!(circular.fields["location_count"], 4);
    assert_eq!(circular.fields["positions"][0], 1);
    assert_eq!(circular.fields["positions"][1], 3);
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_distinguishes_all_external_reference_forms_without_resolution() {
    let bytes = external_reference_forms_file();
    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(&bytes),
            &cadmpeg_ir::decode::InspectOptions::default(),
        )
        .unwrap();
    assert!(summary
        .notes
        .iter()
        .any(|note| note == "external_references=5"));
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let references = &result.ir.native.namespace("iges").unwrap().arenas["external_references"];
    assert_eq!(references.len(), 5);
    assert_eq!(
        references[0].fields["reference_kind"],
        "external_definition"
    );
    assert_eq!(
        references[1].fields["reference_kind"],
        "external_file_definition"
    );
    assert!(references[1].fields["symbolic_name"].is_null());
    assert_eq!(references[2].fields["reference_kind"], "external_logical");
    assert_eq!(references[3].fields["reference_kind"], "native_definition");
    assert_eq!(
        references[4].fields["reference_kind"],
        "native_library_definition"
    );
    assert_eq!(references[4].fields["library_name"][0], 68);
    assert!(references
        .iter()
        .all(|reference| reference.fields["resolution_state"] == "not_attempted"));
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_preserves_group_order_and_back_pointer_policy() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(group_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let groups = &result.ir.native.namespace("iges").unwrap().arenas["groups"];
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].fields["ordered"], true);
    assert_eq!(groups[0].fields["back_pointers_required"], true);
    assert_eq!(groups[0].fields["members"][0], "iges:entity:directory#1");
    assert_eq!(groups[1].fields["ordered"], false);
    assert_eq!(groups[1].fields["back_pointers_required"], false);
    let entities = &result.ir.native.namespace("iges").unwrap().arenas["entities"];
    assert_eq!(
        entities[0].fields["association_links"][0],
        "iges:entity:directory#3"
    );
    assert!(entities[0].fields["property_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_types_all_attribute_table_definition_forms() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(attribute_definition_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let definitions =
        &result.ir.native.namespace("iges").unwrap().arenas["attribute_table_definitions"];
    assert_eq!(definitions.len(), 3);
    assert_eq!(definitions[0].fields["form"], 0);
    assert_eq!(
        definitions[0].fields["attributes"][0]["declared_value_count"],
        1
    );
    assert_eq!(
        definitions[1].fields["attributes"][0]["values"][0]["value"]["kind"],
        "integer"
    );
    assert_eq!(
        definitions[1].fields["attributes"][1]["values"][0]["value"]["kind"],
        "string"
    );
    assert_eq!(
        definitions[2].fields["attributes"][0]["values"][0]["value"]["kind"],
        "real"
    );
    assert!(definitions[2].fields["attributes"][0]["values"][0]["display_template"].is_null());
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_bounds_declared_attribute_counts_by_record_tokens() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 322,
        form: 0,
        label: "BADCOUNT".into(),
        status: "00000200",
        parameters: "322,,0,9223372036854775807;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    let definitions =
        &result.ir.native.namespace("iges").unwrap().arenas["attribute_table_definitions"];
    assert_eq!(definitions.len(), 1);
    assert!(definitions[0].fields["attributes"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("attribute-table definition")));
}

#[test]
fn decode_bounds_declared_brep_counts_by_record_tokens() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 502,
        form: 1,
        label: "BADCOUNT".into(),
        status: "00010000",
        parameters: "502,9223372036854775807;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("vertex-list count")));
}

#[test]
fn decode_bounds_declared_trimming_counts_by_record_tokens() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 108,
            form: 0,
            label: "PLANE".into(),
            status: "00010000",
            parameters: "108,0,0,1,0,0,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 141,
            form: 0,
            label: "BADCOUNT".into(),
            status: "00010000",
            parameters: "141,0,1,1,9223372036854775807;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("boundary segment count")));
}

#[test]
fn decode_bounds_declared_presentation_counts_by_record_tokens() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 310,
        form: 0,
        label: "BADCOUNT".into(),
        status: "00000200",
        parameters: "310,1,1HA,,1,9223372036854775807;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("font header")));
    let fonts = &result.ir.native.namespace("iges").unwrap().arenas["text_fonts"];
    assert!(fonts[0].fields["characters"].as_array().unwrap().is_empty());
}

#[test]
fn decode_bounds_declared_annotation_counts_by_record_tokens() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 212,
        form: 0,
        label: "BADCOUNT".into(),
        status: "00010100",
        parameters: "212,9223372036854775807;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("text count")));
    let annotations = &result.ir.native.namespace("iges").unwrap().arenas["annotations"];
    assert!(annotations[0].fields["strings"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn decode_bounds_declared_drawing_counts_by_record_tokens() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 404,
        form: 0,
        label: "BADCOUNT".into(),
        status: "00000000",
        parameters: "404,9223372036854775807;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("drawing view placements")));
    let drawings = &result.ir.native.namespace("iges").unwrap().arenas["drawings"];
    assert!(drawings[0].fields["views"].as_array().unwrap().is_empty());
}

#[test]
fn decode_bounds_declared_solid_counts_by_record_tokens() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 180,
        form: 0,
        label: "BADCOUNT".into(),
        status: "00000000",
        parameters: "180,9223372036854775807;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("Boolean postfix length")));
    let trees = &result.ir.native.namespace("iges").unwrap().arenas["boolean_trees"];
    assert!(trees[0].fields["terms"].as_array().unwrap().is_empty());
}

#[test]
fn decode_types_attribute_table_tuple_and_row_major_instances() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(attribute_instance_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let instances =
        &result.ir.native.namespace("iges").unwrap().arenas["attribute_table_instances"];
    assert_eq!(instances.len(), 2);
    assert_eq!(
        instances[0].fields["definition"],
        "iges:product:attribute-definition#D1"
    );
    assert_eq!(instances[0].fields["rows"].as_array().unwrap().len(), 1);
    assert_eq!(instances[1].fields["declared_row_count"], 2);
    assert_eq!(instances[1].fields["rows"].as_array().unwrap().len(), 2);
    assert_eq!(instances[1].fields["rows"][1][0]["kind"], "integer");
    assert_eq!(instances[1].fields["rows"][1][1]["kind"], "string");
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_links_product_names_and_reference_designators_to_owners() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(product_property_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let properties = &result.ir.native.namespace("iges").unwrap().arenas["product_properties"];
    assert_eq!(properties.len(), 2);
    assert_eq!(
        properties[0].fields["property_kind"],
        "reference_designator"
    );
    assert_eq!(properties[0].fields["owners"][0], "iges:entity:directory#1");
    assert_eq!(properties[1].fields["property_kind"], "name");
    assert_eq!(properties[1].fields["value"][0], 66);
    assert_eq!(properties[1].fields["owners"][0], "iges:entity:directory#1");
    let owner = &result.ir.native.namespace("iges").unwrap().arenas["entities"][0];
    assert!(owner.fields["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(owner.fields["property_links"][0], "iges:entity:directory#3");
    assert_eq!(owner.fields["property_links"][1], "iges:entity:directory#5");
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_types_scalar_and_string_property_forms() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(scalar_property_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let properties = &result.ir.native.namespace("iges").unwrap().arenas["properties"];
    assert_eq!(properties.len(), 14);
    assert!(properties
        .iter()
        .all(|property| property.id.starts_with("iges:application:property#D")));
    let property = |form| {
        properties
            .iter()
            .find(|property| property.fields["form"] == form)
            .unwrap()
    };
    assert_eq!(property(2).fields["property_kind"], "region_restriction");
    assert_eq!(property(2).fields["electrical_circuitry"], 2);
    assert_eq!(property(5).fields["extension_flag"], 2);
    assert_eq!(property(12).fields["names"].as_array().unwrap().len(), 2);
    assert_eq!(property(13).fields["standard"][0], 65);
    assert_eq!(property(18).fields["percent"], 12.5);
    assert_eq!(property(20).fields["highlighted"], true);
    assert_eq!(property(21).fields["pickable"], true);
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_types_grid_group_and_lep_property_forms() {
    let decode = |bytes| {
        IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap()
    };
    let grid = decode(grid_property_file());
    let property = &grid.ir.native.namespace("iges").unwrap().arenas["properties"][0];
    assert_eq!(property.fields["property_kind"], "uniform_rectangular_grid");
    assert_eq!(property.fields["owners"][0], "iges:entity:directory#5");
    assert!(grid.report.losses.is_empty(), "{:#?}", grid.report.losses);

    let group = decode(group_type_property_file());
    let property = &group.ir.native.namespace("iges").unwrap().arenas["properties"][0];
    assert_eq!(property.fields["associativity_type"], 5);
    assert_eq!(property.fields["owners"][0], "iges:entity:directory#3");
    assert!(group.report.losses.is_empty(), "{:#?}", group.report.losses);

    let lep = decode(lep_property_forms_file());
    let properties = &lep.ir.native.namespace("iges").unwrap().arenas["properties"];
    let property = |form| {
        properties
            .iter()
            .find(|value| value.fields["form"] == form)
            .unwrap()
    };
    assert_eq!(
        property(24).fields["definitions"].as_array().unwrap().len(),
        2
    );
    assert_eq!(property(25).fields["levels"].as_array().unwrap().len(), 3);
    assert_eq!(property(26).fields["function_code"], 5);
    assert_eq!(property(26).fields["owners"][0], "iges:entity:directory#5");
    assert!(lep.report.losses.is_empty(), "{:#?}", lep.report.losses);
}

#[test]
fn decode_types_tabular_and_generic_data_properties() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(variable_schema_property_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let properties = &result.ir.native.namespace("iges").unwrap().arenas["properties"];
    let property = |form| {
        properties
            .iter()
            .find(|value| value.fields["form"] == form)
            .unwrap()
    };
    assert_eq!(property(11).fields["property_kind"], "tabular_data");
    assert_eq!(
        property(11).fields["independent_variables"][0]["values"][1],
        25.0
    );
    assert_eq!(property(11).fields["dependent_values"][1], 46.0);
    assert_eq!(property(27).fields["values"].as_array().unwrap().len(), 6);
    assert_eq!(property(27).fields["values"][4]["value"]["kind"], "integer");
    assert_eq!(property(27).fields["owners"][0], "iges:entity:directory#1");
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_types_dimension_drawing_text_and_closure_properties() {
    let decode = |bytes| {
        IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap()
    };
    for (bytes, expected_forms) in [
        (dimension_property_forms_file(), vec![28, 29, 30, 31]),
        (drawing_metadata_property_forms_file(), vec![32, 33]),
        (text_score_property_forms_file(), vec![34, 35]),
        (closure_property_file(), vec![36]),
    ] {
        let result = decode(bytes);
        let properties = &result.ir.native.namespace("iges").unwrap().arenas["properties"];
        for form in &expected_forms {
            assert!(properties
                .iter()
                .any(|property| property.fields["form"] == *form));
        }
        assert!(
            result.report.losses.is_empty(),
            "forms {expected_forms:?}: {:#?}",
            result.report.losses
        );
    }
}

#[test]
fn decode_types_orthographic_and_perspective_views() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(view_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let views = &result.ir.native.namespace("iges").unwrap().arenas["views"];
    assert_eq!(views.len(), 2);
    assert_eq!(views[0].fields["projection"], "orthographic_parallel");
    assert!(views[0].fields["scale"].is_null());
    assert_eq!(
        views[0].fields["clipping_planes"].as_array().unwrap().len(),
        6
    );
    assert_eq!(views[1].fields["projection"], "perspective");
    assert_eq!(views[1].fields["view_plane_normal"][2], 1.0);
    assert_eq!(views[1].fields["center_of_projection"][2], 10.0);
    assert_eq!(views[1].fields["clipping_window"][0], -2.0);
    assert_eq!(views[1].fields["depth_clipping"], 3);
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_types_view_visibility_and_display_overrides() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(view_visibility_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let visibility = &result.ir.native.namespace("iges").unwrap().arenas["view_visibility"];
    assert_eq!(visibility.len(), 2);
    assert_eq!(visibility[0].fields["form"], 3);
    assert_eq!(
        visibility[0].fields["displays"][0]["view"],
        "iges:presentation:view#D1"
    );
    assert!(visibility[0].fields["displays"][0]["line_font"].is_null());
    assert_eq!(visibility[1].fields["form"], 4);
    assert_eq!(visibility[1].fields["displays"][0]["line_font"], 1);
    assert_eq!(visibility[1].fields["displays"][0]["color"], 2);
    assert_eq!(visibility[1].fields["displays"][0]["line_weight"], 3);
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_preserves_ordered_segmented_view_display() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(segmented_view_visibility_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let segmented = &result.ir.native.namespace("iges").unwrap().arenas["segmented_visibility"][0];
    assert_eq!(segmented.fields["blocks"].as_array().unwrap().len(), 2);
    assert_eq!(segmented.fields["blocks"][0]["breakpoint"], 0.5);
    assert_eq!(segmented.fields["blocks"][0]["color"]["kind"], "omitted");
    assert_eq!(segmented.fields["blocks"][1]["breakpoint"], 1.0);
    assert_eq!(segmented.fields["blocks"][1]["color"]["value"], 2);
    assert_eq!(segmented.fields["blocks"][1]["line_font"]["value"], 3);
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_types_drawing_view_placement_annotations_and_sheet_properties() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(drawing_with_properties_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let drawing = &result.ir.native.namespace("iges").unwrap().arenas["drawings"][0];
    assert_eq!(drawing.fields["form"], 1);
    assert_eq!(
        drawing.fields["views"][0]["view"],
        "iges:presentation:view#D1"
    );
    assert_eq!(drawing.fields["views"][0]["origin"][0], 10.0);
    assert_eq!(drawing.fields["views"][0]["rotation"], 0.5);
    assert_eq!(drawing.fields["annotations"][0], "iges:entity:directory#3");
    assert_eq!(drawing.fields["size"][0], 210.0);
    assert_eq!(drawing.fields["size"][1], 297.0);
    assert_eq!(drawing.fields["units_flag"], 2);
    assert_eq!(drawing.fields["units_name"][0], 77);
    assert_eq!(drawing.fields["name"][0], 68);
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_preserves_general_note_text_runs_and_new_note_control_codes() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(text_annotation_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let annotations = &result.ir.native.namespace("iges").unwrap().arenas["annotations"];
    assert_eq!(annotations.len(), 2);
    assert_eq!(annotations[0].fields["kind"], "general_note");
    assert_eq!(
        annotations[0].fields["strings"].as_array().unwrap().len(),
        2
    );
    assert_eq!(annotations[0].fields["strings"][0]["text"][0], 65);
    assert_eq!(annotations[0].fields["strings"][1]["mirror"], 1);
    assert_eq!(annotations[0].fields["strings"][1]["vertical"], 1);
    assert_eq!(annotations[1].fields["kind"], "new_general_note");
    assert_eq!(annotations[1].fields["justification"], 2);
    assert_eq!(annotations[1].fields["strings"][0]["control_codes"][0], 84);
    assert_eq!(annotations[1].fields["strings"][0]["text"]["text"][3], 33);
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_types_every_leader_arrow_form_and_segment_chain() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(leader_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let annotations = &result.ir.native.namespace("iges").unwrap().arenas["annotations"];
    assert_eq!(annotations.len(), 12);
    let mut forms = Vec::new();
    for annotation in annotations {
        assert_eq!(annotation.fields["kind"], "leader");
        forms.push(annotation.fields["form"].as_i64().unwrap());
        assert_eq!(annotation.fields["arrowhead"][2], 3.0);
        assert_eq!(
            annotation.fields["segment_tails"].as_array().unwrap().len(),
            2
        );
        assert_eq!(annotation.fields["segment_tails"][1][1], 4.0);
    }
    forms.sort_unstable();
    assert_eq!(forms, (1..=12).collect::<Vec<_>>());
    let no_arrow = annotations
        .iter()
        .find(|annotation| annotation.fields["form"] == 4)
        .unwrap();
    assert_eq!(no_arrow.fields["arrowhead_size"][0], 0.0);
    let circle = annotations
        .iter()
        .find(|annotation| annotation.fields["form"] == 5)
        .unwrap();
    assert_eq!(circle.fields["arrowhead_size"][0], 2.0);
    assert_eq!(circle.fields["arrowhead_size"][1], 2.0);
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_types_dimension_component_roles_for_every_admitted_form() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(dimension_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let annotations = &result.ir.native.namespace("iges").unwrap().arenas["annotations"];
    let kinds = annotations
        .iter()
        .filter_map(|annotation| annotation.fields["kind"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == "linear_dimension")
            .count(),
        3
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == "ordinate_dimension")
            .count(),
        2
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == "point_dimension")
            .count(),
        1
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == "radius_dimension")
            .count(),
        2
    );
    let point = annotations
        .iter()
        .find(|annotation| annotation.fields["kind"] == "point_dimension")
        .unwrap();
    assert_eq!(point.fields["note"], "iges:presentation:annotation#D1");
    assert_eq!(point.fields["leader"], "iges:presentation:annotation#D3");
    assert_eq!(point.fields["enclosure"], "iges:entity:directory#7");
    let radius = annotations
        .iter()
        .find(|annotation| {
            annotation.fields["kind"] == "radius_dimension" && annotation.fields["form"] == 1
        })
        .unwrap();
    assert_eq!(radius.fields["center"][0], 10.0);
    assert_eq!(
        radius.fields["leaders"][1],
        "iges:presentation:annotation#D9"
    );
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_types_angular_curve_diameter_flag_and_label_annotations() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(legacy_dimension_and_label_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let annotations = &result.ir.native.namespace("iges").unwrap().arenas["annotations"];
    for kind in [
        "angular_dimension",
        "curve_dimension",
        "diameter_dimension",
        "flag_note",
        "general_label",
    ] {
        assert!(annotations
            .iter()
            .any(|annotation| annotation.fields["kind"] == kind));
    }
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_types_general_symbol_components_and_section_fill_definition() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(symbol_and_sectioned_area_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let annotations = &result.ir.native.namespace("iges").unwrap().arenas["annotations"];
    let symbol = annotations
        .iter()
        .find(|annotation| annotation.fields["kind"] == "general_symbol")
        .unwrap();
    assert_eq!(symbol.fields["note"], "iges:presentation:annotation#D1");
    assert_eq!(symbol.fields["geometry"][0], "iges:entity:directory#3");
    assert_eq!(
        symbol.fields["leaders"][0],
        "iges:presentation:annotation#D5"
    );
    let section = annotations
        .iter()
        .find(|annotation| annotation.fields["kind"] == "sectioned_area")
        .unwrap();
    assert_eq!(section.fields["boundary"], "iges:entity:directory#9");
    assert_eq!(section.fields["fill_pattern"], 2);
    assert_eq!(section.fields["pattern_spacing"], 1.0);
    assert_eq!(section.fields["islands"][0], "iges:entity:directory#11");
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_preserves_implementor_associativity_class_grammar() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(associativity_definition_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let definition = &result.ir.native.namespace("iges").unwrap().arenas["associativities"][0];
    assert_eq!(definition.fields["kind"], "definition");
    assert_eq!(definition.fields["associativity_form"], 5001);
    assert_eq!(definition.fields["classes"].as_array().unwrap().len(), 2);
    assert_eq!(
        definition.fields["classes"][0]["back_pointers_required"],
        true
    );
    assert_eq!(definition.fields["classes"][0]["ordered"], true);
    assert_eq!(definition.fields["classes"][0]["item_types"][0], 1);
    assert_eq!(definition.fields["classes"][0]["item_types"][1], 2);
    assert_eq!(
        definition.fields["classes"][1]["back_pointers_required"],
        false
    );
    assert_eq!(definition.fields["classes"][1]["ordered"], false);
    assert_eq!(definition.fields["classes"][1]["item_types"][0], 3);
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_types_bounded_predefined_associativity_roles() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(bounded_associativity_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let associativities = &result.ir.native.namespace("iges").unwrap().arenas["associativities"];
    assert_eq!(associativities.len(), 5);
    let parent = associativities
        .iter()
        .find(|value| value.fields["kind"] == "single_parent")
        .unwrap();
    assert_eq!(parent.fields["parent"], "iges:entity:directory#9");
    assert_eq!(parent.fields["children"][0], "iges:entity:directory#11");
    let labels = associativities
        .iter()
        .find(|value| value.fields["kind"] == "label_display")
        .unwrap();
    assert_eq!(
        labels.fields["placements"][0]["view"],
        "iges:entity:directory#1"
    );
    assert_eq!(labels.fields["placements"][0]["text_location"][2], 3.0);
    let dimension = associativities
        .iter()
        .find(|value| value.fields["kind"] == "dimensioned_geometry")
        .unwrap();
    assert_eq!(dimension.fields["dimension"], "iges:entity:directory#21");
    assert_eq!(dimension.fields["geometry"][0], "iges:entity:directory#9");
    let planar = associativities
        .iter()
        .find(|value| value.fields["kind"] == "planar")
        .unwrap();
    assert!(planar.fields["plane_transform"].is_null());
    assert_eq!(planar.fields["entities"].as_array().unwrap().len(), 2);
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_types_view_list_with_required_back_pointers() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(view_list_associativity_file(true)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let view_list = result.ir.native.namespace("iges").unwrap().arenas["associativities"]
        .iter()
        .find(|value| value.fields["kind"] == "view_list")
        .unwrap();
    assert_eq!(view_list.fields["declared_visible_count"], 1);
    assert_eq!(view_list.fields["view"], "iges:entity:directory#1");
    assert_eq!(
        view_list.fields["visible_entities"][0],
        "iges:entity:directory#5"
    );
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );

    let missing = IgesCodec
        .decode(
            &mut Cursor::new(view_list_associativity_file(false)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(missing.report.losses.iter().any(|loss| {
        loss.message.contains("entity type 402 form 6")
            && loss.message.contains("predefined associativity")
    }));
}

#[test]
fn decode_preserves_signal_and_piping_flow_class_order() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(flow_associativity_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let associativities = &result.ir.native.namespace("iges").unwrap().arenas["associativities"];
    let signal = associativities
        .iter()
        .find(|value| {
            value.fields["kind"] == "flow"
                && value.fields["form"] == 18
                && value.fields["connections"].as_array().unwrap().len() == 1
        })
        .unwrap();
    assert_eq!(signal.fields["type_flag"], 1);
    assert_eq!(signal.fields["function_flag"], 2);
    assert_eq!(signal.fields["connections"][0], "iges:entity:directory#1");
    assert_eq!(signal.fields["joins"][0], "iges:entity:directory#3");
    assert_eq!(signal.fields["names"][0][0], 70);
    assert_eq!(signal.fields["name_displays"][0], "iges:entity:directory#5");
    assert_eq!(signal.fields["continuations"][0], "iges:entity:directory#9");
    let pipe = associativities
        .iter()
        .find(|value| {
            value.fields["kind"] == "flow"
                && value.fields["form"] == 20
                && value.fields["connections"].as_array().unwrap().len() == 1
        })
        .unwrap();
    assert_eq!(pipe.fields["type_flag"], 2);
    assert!(pipe.fields["function_flag"].is_null());
    assert_eq!(pipe.fields["connections"][0], "iges:entity:directory#11");
    assert_eq!(pipe.fields["continuations"][0], "iges:entity:directory#17");
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_preserves_recalculable_dimension_geometry_points() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(recalculable_dimension_associativity_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let associativity = result.ir.native.namespace("iges").unwrap().arenas["associativities"]
        .iter()
        .find(|value| value.fields["kind"] == "recalculable_dimension")
        .unwrap();
    assert_eq!(
        associativity.fields["dimension"],
        "iges:entity:directory#11"
    );
    assert_eq!(associativity.fields["orientation_flag"], 4);
    assert_eq!(
        associativity.fields["geometry"].as_array().unwrap().len(),
        2
    );
    assert_eq!(
        associativity.fields["geometry"][0]["geometry"],
        "iges:entity:directory#7"
    );
    assert_eq!(associativity.fields["geometry"][0]["location_flag"], 0);
    assert_eq!(
        associativity.fields["geometry"][1]["geometry"],
        "iges:entity:directory#9"
    );
    assert_eq!(associativity.fields["geometry"][1]["location_flag"], 1);
    assert_eq!(associativity.fields["geometry"][1]["point"][0], 4.0);
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_distinguishes_absolute_and_incremental_text_templates() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(text_display_template_forms_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let templates = &result.ir.native.namespace("iges").unwrap().arenas["text_templates"];
    assert_eq!(templates.len(), 2);
    let absolute = templates
        .iter()
        .find(|template| template.fields["form"] == 0)
        .unwrap();
    assert_eq!(absolute.fields["origin_or_increment"][0], 10.0);
    assert_eq!(absolute.fields["origin_or_increment"][1], 20.0);
    let incremental = templates
        .iter()
        .find(|template| template.fields["form"] == 1)
        .unwrap();
    assert_eq!(incremental.fields["font_code"], 18);
    assert_eq!(incremental.fields["mirror"], 1);
    assert_eq!(incremental.fields["vertical"], 1);
    assert_eq!(incremental.fields["origin_or_increment"][0], 2.0);
    assert_eq!(incremental.fields["origin_or_increment"][1], -1.0);
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_preserves_text_font_glyphs_and_supersession() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(text_font_definition_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let fonts = &result.ir.native.namespace("iges").unwrap().arenas["text_fonts"];
    assert_eq!(fonts.len(), 2);
    let base = fonts
        .iter()
        .find(|font| font.fields["font_code"] == 101)
        .unwrap();
    assert_eq!(base.fields["characters"].as_array().unwrap().len(), 2);
    assert_eq!(base.fields["characters"][0]["character_code"], 65);
    assert_eq!(
        base.fields["characters"][0]["motions"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(base.fields["characters"][0]["motions"][0]["pen_up"].is_null());
    assert_eq!(base.fields["characters"][0]["motions"][1]["pen_up"], false);
    assert_eq!(base.fields["characters"][1]["declared_motion_count"], 0);
    let modification = fonts
        .iter()
        .find(|font| font.fields["font_code"] == 102)
        .unwrap();
    assert_eq!(
        modification.fields["supersedes_definition"],
        "iges:presentation:text-font#D1"
    );
    assert_eq!(
        modification.fields["characters"][0]["motions"][0]["pen_up"],
        true
    );
    let template = &result.ir.native.namespace("iges").unwrap().arenas["text_templates"][0];
    assert_eq!(
        template.fields["font_definition"],
        "iges:presentation:text-font#D3"
    );
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_types_fundamental_units_and_property_owner() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(units_data_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let units = &result.ir.native.namespace("iges").unwrap().arenas["units_data"][0];
    assert_eq!(units.fields["units"].as_array().unwrap().len(), 3);
    assert_eq!(units.fields["units"][0]["unit_type"][0], 76);
    assert_eq!(units.fields["units"][0]["unit_value"][0], 77);
    assert_eq!(units.fields["units"][0]["scale_factor"], 1000.0);
    assert_eq!(
        units.fields["units"][2]["scale_factor"],
        0.017_453_292_519_943_295
    );
    assert_eq!(units.fields["owners"][0], "iges:entity:directory#1");
    let owner = &result.ir.native.namespace("iges").unwrap().arenas["entities"][0];
    assert_eq!(owner.fields["property_links"][0], "iges:entity:directory#3");
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_preserves_ordered_solid_assembly_member_placements() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(solid_assembly_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let assemblies = &result.ir.native.namespace("iges").unwrap().arenas["solid_assemblies"];
    assert_eq!(assemblies.len(), 1);
    let items = assemblies[0].fields["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["item"], "iges:entity:directory#1");
    assert!(items[0]["transformation"].is_null());
    assert_eq!(items[1]["item"], "iges:entity:directory#3");
    assert_eq!(items[1]["transformation"], "iges:native:transformation#D5");
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_rejects_cyclic_solid_assembly_definitions() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 158,
            form: 0,
            label: "SPHERE".into(),
            status: "00000000",
            parameters: "158,1,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 184,
            form: 0,
            label: "ASSEMBL1".into(),
            status: "00000200",
            parameters: "184,2,1,5,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 184,
            form: 0,
            label: "ASSEMBL2".into(),
            status: "00000200",
            parameters: "184,2,1,3,0,0;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        result.ir.native.namespace("iges").unwrap().arenas["solid_assemblies"].len(),
        2
    );
    assert_eq!(
        result
            .report
            .losses
            .iter()
            .filter(|loss| loss.message.contains(
                "solid-assembly use flag, form, members, transforms, or acyclicity is invalid"
            ))
            .count(),
        2
    );
}

#[test]
fn decode_preserves_nested_subfigure_definitions_and_instances() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(nested_subfigure_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir.native.namespace("iges").unwrap();
    let definitions = &native.arenas["subfigure_definitions"];
    assert_eq!(definitions.len(), 2);
    let parent = definitions
        .iter()
        .find(|definition| definition.id == "iges:product:subfigure-definition#D7")
        .unwrap();
    assert_eq!(parent.fields["depth"], 1);
    assert_eq!(parent.fields["members"][0], "iges:entity:directory#5");
    let instances = &native.arenas["subfigure_instances"];
    assert_eq!(instances.len(), 2);
    let child = instances
        .iter()
        .find(|instance| instance.id == "iges:product:subfigure-instance#D5")
        .unwrap();
    assert_eq!(
        child.fields["definition"],
        "iges:product:subfigure-definition#D3"
    );
    assert_eq!(child.fields["translation"][0], 1.0);
    assert_eq!(child.fields["scale"], 0.5);
    let occurrences = &native.arenas["product_occurrences"];
    assert_eq!(occurrences.len(), 3);
    let nested = occurrences
        .iter()
        .find(|occurrence| occurrence.id == "iges:product:occurrence#9/5")
        .unwrap();
    assert_eq!(nested.fields["instance_path"][0], "iges:entity:directory#9");
    assert_eq!(nested.fields["instance_path"][1], "iges:entity:directory#5");
    assert_eq!(nested.fields["world_transform"][0][0], 1.0);
    assert_eq!(nested.fields["world_transform"][0][3], 12.0);
    assert_eq!(nested.fields["world_transform"][1][3], 24.0);
    assert_eq!(nested.fields["world_transform"][2][3], 36.0);
    let leaf = occurrences
        .iter()
        .find(|occurrence| occurrence.id == "iges:product:occurrence#9/5/D1")
        .unwrap();
    assert_eq!(leaf.fields["member"], "iges:entity:directory#1");
    assert_eq!(leaf.fields["neutral_links"][0], "iges:model:curve#D1");
    assert_eq!(
        leaf.fields["world_transform"],
        nested.fields["world_transform"]
    );
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_bounds_product_occurrence_expansion_with_a_named_loss() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(occurrence_limit_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir.native.namespace("iges").unwrap();

    assert_eq!(native.arenas["product_occurrences"].len(), 100);
    let expansion = &native.arenas["product_occurrence_expansion"][0];
    assert_eq!(expansion.fields["limit"], 100);
    assert_eq!(expansion.fields["emitted"], 100);
    assert_eq!(expansion.fields["truncated"], true);
    assert!(result.report.losses.iter().any(|loss| {
        loss.message == "IGES product occurrence expansion reached its configured output limit"
    }));
}

#[test]
fn decode_rejects_non_decreasing_subfigure_nesting_depth() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(invalid_subfigure_depth_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result.report.losses.iter().any(|loss| loss
        .message
        .contains("subfigure definition fields or nesting depth is invalid")));
    assert_eq!(
        result.ir.native.namespace("iges").unwrap().arenas["subfigure_definitions"].len(),
        2
    );
}

#[test]
fn decode_preserves_network_definition_and_anisotropic_instance() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(network_subfigure_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir.native.namespace("iges").unwrap();
    let definition = &native.arenas["network_definitions"][0];
    assert_eq!(definition.id, "iges:product:network-definition#D1");
    assert_eq!(definition.fields["type_flag"], 1);
    assert_eq!(definition.fields["declared_connect_point_count"], 2);
    assert_eq!(
        definition.fields["connect_points"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let instance = &native.arenas["network_instances"][0];
    assert_eq!(
        instance.fields["definition"],
        "iges:product:network-definition#D1"
    );
    assert_eq!(instance.fields["translation"][2], 3.0);
    assert_eq!(instance.fields["scale"][0], 2.0);
    assert!(instance.fields["scale"][1].is_null());
    assert!(instance.fields["scale"][2].is_null());
    let occurrence = &native.arenas["product_occurrences"][0];
    assert_eq!(occurrence.fields["world_transform"][0][0], 2.0);
    assert_eq!(occurrence.fields["world_transform"][1][1], 2.0);
    assert_eq!(occurrence.fields["world_transform"][2][2], 2.0);
    assert_eq!(occurrence.fields["world_transform"][0][3], 1.0);
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_preserves_owned_network_connect_points() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(connected_network_subfigure_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir.native.namespace("iges").unwrap();
    let points = &native.arenas["connect_points"];
    assert_eq!(points.len(), 2);
    assert_eq!(points[0].fields["type_flag"], 101);
    assert_eq!(points[0].fields["function_identifier"][0], 80);
    assert_eq!(points[0].fields["function_identifier"][1], 49);
    assert_eq!(points[0].fields["owner"], "iges:entity:directory#3");
    assert_eq!(points[1].fields["position"][2], 3.0);
    assert_eq!(points[1].fields["owner"], "iges:entity:directory#7");
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_rejects_cyclic_boolean_tree_references() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 158,
            form: 0,
            label: "SPHERE".into(),
            status: "00000000",
            parameters: "158,1,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 180,
            form: 0,
            label: "TREE1".into(),
            status: "00000000",
            parameters: "180,3,-1,-5,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 180,
            form: 0,
            label: "TREE2".into(),
            status: "00000000",
            parameters: "180,3,-1,-3,1;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        result.ir.native.namespace("iges").unwrap().arenas["boolean_trees"].len(),
        2
    );
    assert_eq!(
        result
            .report
            .losses
            .iter()
            .filter(|loss| loss
                .message
                .contains("Boolean operands, form, or reference acyclicity is invalid"))
            .count(),
        2
    );
}

fn append_tetrahedral_shell(
    entities: &mut Vec<OwnedTestEntity>,
    label: &str,
    origin: [f64; 3],
    size: f64,
) -> u32 {
    let sequence = |index: usize| u32::try_from(index * 2 + 1).unwrap();
    let first = entities.len();
    let vertices = [
        origin,
        [origin[0] + size, origin[1], origin[2]],
        [origin[0], origin[1] + size, origin[2]],
        [origin[0], origin[1], origin[2] + size],
    ];
    for (index, point) in vertices.iter().enumerate().take(2) {
        entities.push(OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: format!("{label}P{index}"),
            status: "00010000",
            parameters: format!("116,{},{},{},0;", point[0], point[1], point[2]),
        });
    }
    for (index, normal) in [
        [0.0, 0.0, -1.0],
        [0.0, -1.0, 0.0],
        [-1.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
    ]
    .iter()
    .enumerate()
    {
        entities.push(OwnedTestEntity {
            entity_type: 123,
            form: 0,
            label: format!("{label}N{index}"),
            status: "00010000",
            parameters: format!("123,{},{},{};", normal[0], normal[1], normal[2]),
        });
    }
    for (index, (point_offset, normal_offset)) in
        [(0, 2), (0, 3), (0, 4), (1, 5)].into_iter().enumerate()
    {
        entities.push(OwnedTestEntity {
            entity_type: 190,
            form: 0,
            label: format!("{label}S{index}"),
            status: "00010000",
            parameters: format!(
                "190,{},{};",
                sequence(first + point_offset),
                sequence(first + normal_offset)
            ),
        });
    }
    for (index, (start, end)) in [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)]
        .into_iter()
        .enumerate()
    {
        let a = vertices[start];
        let b = vertices[end];
        entities.push(OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: format!("{label}E{index}"),
            status: "00010000",
            parameters: format!("110,{},{},{},{},{},{};", a[0], a[1], a[2], b[0], b[1], b[2]),
        });
    }
    let vertex_list = sequence(entities.len());
    entities.push(OwnedTestEntity {
        entity_type: 502,
        form: 1,
        label: format!("{label}VERT"),
        status: "00010000",
        parameters: format!(
            "502,4,{},{},{},{},{},{},{},{},{},{},{},{};",
            vertices[0][0],
            vertices[0][1],
            vertices[0][2],
            vertices[1][0],
            vertices[1][1],
            vertices[1][2],
            vertices[2][0],
            vertices[2][1],
            vertices[2][2],
            vertices[3][0],
            vertices[3][1],
            vertices[3][2]
        ),
    });
    let edge_list = sequence(entities.len());
    let curve = |offset: usize| sequence(first + 10 + offset);
    entities.push(OwnedTestEntity {
        entity_type: 504,
        form: 1,
        label: format!("{label}EDGE"),
        status: "00010001",
        parameters: format!(
            "504,6,{}, {},1,{},2,{}, {},1,{},3,{}, {},1,{},4,{}, {},2,{},3,{}, {},2,{},4,{}, {},3,{},4;",
            curve(0), vertex_list, vertex_list,
            curve(1), vertex_list, vertex_list,
            curve(2), vertex_list, vertex_list,
            curve(3), vertex_list, vertex_list,
            curve(4), vertex_list, vertex_list,
            curve(5), vertex_list, vertex_list,
        ).replace(' ', ""),
    });
    let mut loop_sequences = Vec::new();
    for (index, uses) in [
        [(2, 1), (4, 0), (1, 0)],
        [(1, 1), (5, 1), (3, 0)],
        [(3, 1), (6, 0), (2, 0)],
        [(4, 1), (6, 1), (5, 0)],
    ]
    .into_iter()
    .enumerate()
    {
        let loop_sequence = sequence(entities.len());
        loop_sequences.push(loop_sequence);
        entities.push(OwnedTestEntity {
            entity_type: 508,
            form: 1,
            label: format!("{label}L{index}"),
            status: "00010000",
            parameters: format!(
                "508,3,0,{edge_list},{}, {},0,0,{edge_list},{}, {},0,0,{edge_list},{}, {},0;",
                uses[0].0, uses[0].1, uses[1].0, uses[1].1, uses[2].0, uses[2].1
            )
            .replace(' ', ""),
        });
    }
    let mut face_sequences = Vec::new();
    for (index, loop_sequence) in loop_sequences.into_iter().enumerate() {
        let face_sequence = sequence(entities.len());
        face_sequences.push(face_sequence);
        entities.push(OwnedTestEntity {
            entity_type: 510,
            form: 1,
            label: format!("{label}F{index}"),
            status: "00010000",
            parameters: format!("510,{},1,1,{loop_sequence};", sequence(first + 6 + index)),
        });
    }
    let shell = sequence(entities.len());
    entities.push(OwnedTestEntity {
        entity_type: 514,
        form: 1,
        label: format!("{label}SH"),
        status: "00010000",
        parameters: format!(
            "514,4,{},1,{},1,{},1,{},1;",
            face_sequences[0], face_sequences[1], face_sequences[2], face_sequences[3]
        ),
    });
    shell
}

fn explicit_void_solid_file() -> (Vec<u8>, u32, u32, u32) {
    let mut entities = Vec::new();
    let outer = append_tetrahedral_shell(&mut entities, "OUT", [0.0, 0.0, 0.0], 4.0);
    let void = append_tetrahedral_shell(&mut entities, "VOID", [0.5, 0.5, 0.5], 0.5);
    let solid = u32::try_from(entities.len() * 2 + 1).unwrap();
    entities.push(OwnedTestEntity {
        entity_type: 186,
        form: 0,
        label: "VOIDBODY".into(),
        status: "00000000",
        parameters: format!("186,{outer},1,1,{void},0;"),
    });

    (owned_test_file(&entities), solid, outer, void)
}

#[test]
fn decode_builds_a_solid_with_an_oriented_void_shell() {
    let (bytes, solid_sequence, outer_sequence, void_sequence) = explicit_void_solid_file();
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let body = result
        .ir
        .model
        .bodies
        .iter()
        .find(|body| body.id.0 == format!("iges:model:body#D{solid_sequence}"))
        .unwrap();
    assert_eq!(body.kind, cadmpeg_ir::topology::BodyKind::Solid);
    let region = result
        .ir
        .model
        .regions
        .iter()
        .find(|region| region.id == body.regions[0])
        .unwrap();
    assert_eq!(region.shells.len(), 2);
    assert_eq!(
        region.shells[0].0,
        format!("iges:model:shell#D{solid_sequence}:D{outer_sequence}")
    );
    assert_eq!(
        region.shells[1].0,
        format!("iges:model:shell#D{solid_sequence}:D{void_sequence}")
    );
    let void_shell = result
        .ir
        .model
        .shells
        .iter()
        .find(|shell| shell.id == region.shells[1])
        .unwrap();
    for face_id in &void_shell.faces {
        let face = result
            .ir
            .model
            .faces
            .iter()
            .find(|face| face.id == *face_id)
            .unwrap();
        assert_eq!(face.sense, cadmpeg_ir::topology::Sense::Reversed);
    }
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_rejects_closed_shell_with_inconsistent_radial_sense() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file_with_options(false, true)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result
        .ir
        .model
        .bodies
        .iter()
        .all(|body| body.id.0 != "iges:model:body#D55"));
    assert!(result.report.losses.iter().any(|loss| {
        loss.message
            == "IGES entity type 186 form 0 was not projected: closed shell does not use every edge exactly twice with opposite senses"
    }));
    assert_eq!(
        result.ir.native.namespace("iges").unwrap().arenas["entities"].len(),
        28
    );
}

#[test]
fn decode_applies_manifold_solid_placement_at_body_scope_once() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file_with_transform(true)),
            &DecodeOptions::default(),
        )
        .unwrap();

    let body = result
        .ir
        .model
        .bodies
        .iter()
        .find(|body| body.id.0 == "iges:model:body#D55")
        .unwrap();
    assert_eq!(
        body.transform.as_ref().unwrap().rows,
        [
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 20.0],
            [0.0, 0.0, 1.0, 30.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    );
    let points = result
        .ir
        .model
        .points
        .iter()
        .filter(|point| point.id.0.starts_with("iges:model:point#D55:"))
        .map(|point| point.position)
        .collect::<Vec<_>>();
    assert!(points.contains(&cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)));
    assert!(points.contains(&cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0)));
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
}

#[test]
fn decode_builds_a_connected_manifold_tetrahedron() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let body = result
        .ir
        .model
        .bodies
        .iter()
        .find(|body| body.id.0 == "iges:model:body#D55")
        .unwrap();
    assert_eq!(body.kind, cadmpeg_ir::topology::BodyKind::Solid);
    let region = result
        .ir
        .model
        .regions
        .iter()
        .find(|region| region.id == body.regions[0])
        .unwrap();
    assert_eq!(region.shells.len(), 1);
    let shell = result
        .ir
        .model
        .shells
        .iter()
        .find(|shell| shell.id == region.shells[0])
        .unwrap();
    assert_eq!(shell.faces.len(), 4);
    let solid_edges = result
        .ir
        .model
        .edges
        .iter()
        .filter(|edge| edge.id.0.starts_with("iges:model:edge#D55:"))
        .collect::<Vec<_>>();
    assert_eq!(solid_edges.len(), 6);
    for edge in solid_edges {
        let uses = result
            .ir
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.edge == edge.id)
            .collect::<Vec<_>>();
        assert_eq!(uses.len(), 2);
        assert_ne!(uses[0].sense, uses[1].sense);
        assert_eq!(uses[0].radial_next, uses[1].id);
        assert_eq!(uses[1].radial_next, uses[0].id);
    }
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_shared_explicit_open_shell_topology() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(explicit_open_shell_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let body = result
        .ir
        .model
        .bodies
        .iter()
        .find(|body| body.id.0 == "iges:model:body#D23")
        .unwrap();
    assert_eq!(body.kind, cadmpeg_ir::topology::BodyKind::Sheet);
    let shell = result
        .ir
        .model
        .shells
        .iter()
        .find(|shell| shell.id.0 == "iges:model:shell#D23")
        .unwrap();
    assert_eq!(shell.faces.len(), 1);
    let face = result
        .ir
        .model
        .faces
        .iter()
        .find(|face| face.id == shell.faces[0])
        .unwrap();
    let loop_ = result
        .ir
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id == face.loops[0])
        .unwrap();
    assert_eq!(
        loop_.boundary_role,
        cadmpeg_ir::topology::LoopBoundaryRole::Outer
    );
    assert_eq!(loop_.coedges.len(), 4);
    let explicit_edges = result
        .ir
        .model
        .edges
        .iter()
        .filter(|edge| edge.id.0.starts_with("iges:model:edge#D23:"))
        .collect::<Vec<_>>();
    assert_eq!(explicit_edges.len(), 4);
    assert_eq!(
        explicit_edges
            .iter()
            .flat_map(|edge| [&edge.start, &edge.end])
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        4
    );
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_preserves_a_three_use_non_manifold_radial_ring() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(explicit_non_manifold_open_shell_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let edge = result
        .ir
        .model
        .edges
        .iter()
        .find(|edge| edge.id.0 == "iges:model:edge#D37:D23:1")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report.losses));
    let uses = result
        .ir
        .model
        .coedges
        .iter()
        .filter(|coedge| coedge.edge == edge.id)
        .collect::<Vec<_>>();
    assert_eq!(uses.len(), 3);
    let by_id = uses
        .iter()
        .map(|coedge| (&coedge.id, *coedge))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut current = uses[0];
    let mut visited = std::collections::BTreeSet::new();
    for _ in 0..3 {
        assert!(visited.insert(current.id.clone()));
        current = by_id[&current.radial_next];
    }
    assert_eq!(current.id, uses[0].id);
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_a_parametrically_bounded_sheet() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(parametrically_bounded_plane_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let face = result
        .ir
        .model
        .faces
        .iter()
        .find(|face| face.id.0 == "iges:model:face#D9")
        .unwrap();
    let loop_ = result
        .ir
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id == face.loops[0])
        .unwrap();
    let coedge = result
        .ir
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id == loop_.coedges[0])
        .unwrap();
    assert_eq!(
        loop_.boundary_role,
        cadmpeg_ir::topology::LoopBoundaryRole::Unspecified
    );
    assert!(!coedge.pcurves.is_empty());
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_an_ordered_multi_segment_bounded_sheet() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(bounded_plane_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let face = result
        .ir
        .model
        .faces
        .iter()
        .find(|face| face.id.0 == "iges:model:face#D13")
        .unwrap();
    let loop_ = result
        .ir
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id == face.loops[0])
        .unwrap();
    assert_eq!(loop_.coedges.len(), 4);
    let senses = loop_
        .coedges
        .iter()
        .map(|id| {
            result
                .ir
                .model
                .coedges
                .iter()
                .find(|coedge| coedge.id == *id)
                .unwrap()
                .sense
        })
        .collect::<Vec<_>>();
    assert_eq!(
        senses,
        vec![
            cadmpeg_ir::topology::Sense::Forward,
            cadmpeg_ir::topology::Sense::Reversed,
            cadmpeg_ir::topology::Sense::Forward,
            cadmpeg_ir::topology::Sense::Forward,
        ]
    );
    assert!(result
        .ir
        .model
        .coedges
        .iter()
        .filter(|coedge| coedge.owner_loop == loop_.id)
        .all(|coedge| coedge.pcurves.is_empty()));
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_a_valid_face_local_trimmed_sheet() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_plane_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let sheet = result
        .ir
        .model
        .bodies
        .iter()
        .find(|body| body.id.0 == "iges:model:body#D9")
        .unwrap();
    assert_eq!(sheet.kind, cadmpeg_ir::topology::BodyKind::Sheet);
    let face = result
        .ir
        .model
        .faces
        .iter()
        .find(|face| face.id.0 == "iges:model:face#D9")
        .unwrap();
    assert_eq!(face.surface.0, "iges:model:surface#D1");
    assert_eq!(face.loops.len(), 1);
    let loop_ = result
        .ir
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id == face.loops[0])
        .unwrap();
    assert_eq!(
        loop_.boundary_role,
        cadmpeg_ir::topology::LoopBoundaryRole::Outer
    );
    assert_eq!(loop_.coedges.len(), 1);
    let coedge = result
        .ir
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id == loop_.coedges[0])
        .unwrap();
    assert_eq!(coedge.radial_next, coedge.id);
    assert!(!coedge.pcurves.is_empty());
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_a_model_curve_only_trimmed_sheet() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(model_curve_only_trimmed_plane_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let face = result
        .ir
        .model
        .faces
        .iter()
        .find(|face| face.id.0 == "iges:model:face#D9")
        .unwrap();
    let loop_ = result
        .ir
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id == face.loops[0])
        .unwrap();
    let coedge = result
        .ir
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id == loop_.coedges[0])
        .unwrap();
    assert!(coedge.pcurves.is_empty());
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_projects_all_pointer_defined_analytic_surface_forms() {
    for entity_type in [190, 192, 194, 196, 198] {
        for form in [0, 1] {
            let result = IgesCodec
                .decode(
                    &mut Cursor::new(pointer_defined_surface_file(entity_type, form)),
                    &DecodeOptions::default(),
                )
                .unwrap();
            let surface = result
                .ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id.0 == "iges:model:surface#D7")
                .unwrap();
            match (entity_type, &surface.geometry) {
                (190, cadmpeg_ir::geometry::SurfaceGeometry::Plane { origin, .. }) => {
                    assert_eq!(*origin, cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0));
                }
                (192, cadmpeg_ir::geometry::SurfaceGeometry::Cylinder { radius, .. })
                    if *radius == 2.0 => {}
                (
                    194,
                    cadmpeg_ir::geometry::SurfaceGeometry::Cone {
                        radius, half_angle, ..
                    },
                ) if *radius == 2.0
                    && (*half_angle - std::f64::consts::FRAC_PI_6).abs() < 1.0e-15 => {}
                (196, cadmpeg_ir::geometry::SurfaceGeometry::Sphere { radius, .. })
                    if *radius == 2.0 => {}
                (
                    198,
                    cadmpeg_ir::geometry::SurfaceGeometry::Torus {
                        major_radius,
                        minor_radius,
                        ..
                    },
                ) if *major_radius == 4.0 && *minor_radius == 1.0 => {}
                _ => panic!(
                    "unexpected type {entity_type} form {form} projection: {:?}",
                    surface.geometry
                ),
            }
            assert!(cadmpeg_ir::eval::surface_point(&surface.geometry, 0.25, 0.5).is_some());
            assert!(
                result.report.losses.is_empty(),
                "{:#?}",
                result.report.losses
            );
            let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
            assert!(validation.is_ok(), "{:#?}", validation.findings);
        }
    }
}

#[test]
fn decode_solves_signed_analytic_offset_surfaces() {
    for (indicator_z, expected_z) in [(1.0, 2.0), (-1.0, -2.0)] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(offset_plane_file(indicator_z, 2.0)),
                &DecodeOptions::default(),
            )
            .unwrap();

        let offset = result
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.0 == "iges:model:surface#D3")
            .unwrap();
        let cadmpeg_ir::geometry::SurfaceGeometry::Plane { origin, .. } = offset.geometry else {
            panic!("expected an exact plane offset carrier");
        };
        assert_eq!(origin, cadmpeg_ir::math::Point3::new(0.0, 0.0, expected_z));
        assert_eq!(result.ir.model.procedural_surfaces.len(), 1);
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Offset { distance, .. } =
            result.ir.model.procedural_surfaces[0].definition
        else {
            panic!("expected an offset dependency");
        };
        assert_eq!(distance, expected_z);
        assert!(result.report.losses.is_empty());
        let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_projects_a_bspline_surface_with_u_major_control_order() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(nurbs_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs) = &result.ir.model.surfaces[0].geometry
    else {
        panic!("expected a NURBS surface carrier");
    };
    assert_eq!((nurbs.u_degree, nurbs.v_degree), (1, 1));
    assert_eq!((nurbs.u_count, nurbs.v_count), (2, 2));
    assert_eq!(
        nurbs.control_points,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
        ]
    );
    assert_eq!(
        cadmpeg_ir::eval::nurbs_surface_point(nurbs, 0.25, 0.75),
        Some(cadmpeg_ir::math::Point3::new(0.25, 0.75, 0.0))
    );
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_preserves_rational_bspline_weights_and_multiplicities() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(rational_nurbs_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &result.ir.model.curves[0].geometry
    else {
        panic!("expected a NURBS carrier");
    };
    assert_eq!(nurbs.degree, 2);
    assert_eq!(nurbs.knots, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(nurbs.weights, Some(vec![1.0, 0.5, 1.0]));
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point(
            nurbs.degree,
            &nurbs.knots,
            &nurbs.control_points,
            nurbs.weights.as_deref(),
            0.5,
        ),
        Some(cadmpeg_ir::math::Point3::new(1.0, 1.0 / 3.0, 0.0))
    );
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_preserves_a_rational_declaration_with_equal_weights() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(equal_weight_rational_nurbs_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &result.ir.model.curves[0].geometry
    else {
        panic!("expected a NURBS carrier");
    };
    assert_eq!(nurbs.weights, Some(vec![1.0, 1.0, 1.0]));
    assert!(result.report.losses.is_empty());
}

#[test]
fn decode_projects_a_bounded_polynomial_bspline_curve() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(nurbs_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &result.ir.model.curves[0].geometry
    else {
        panic!("expected a NURBS carrier");
    };
    assert_eq!(nurbs.degree, 1);
    assert_eq!(nurbs.knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(nurbs.control_points.len(), 2);
    assert_eq!(nurbs.weights, None);
    assert!(!nurbs.periodic);
    assert_eq!(
        cadmpeg_ir::eval::nurbs_curve_point(
            nurbs.degree,
            &nurbs.knots,
            &nurbs.control_points,
            nurbs.weights.as_deref(),
            0.5,
        ),
        Some(cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0))
    );
    assert_eq!(result.ir.model.edges[0].param_range, Some([0.0, 1.0]));
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_projects_a_counterclockwise_circular_arc() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(circular_arc_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir.model.curves.len(), 1);
    let cadmpeg_ir::geometry::CurveGeometry::Circle {
        center,
        axis,
        ref_direction,
        radius,
    } = &result.ir.model.curves[0].geometry
    else {
        panic!("expected a circle carrier");
    };
    assert_eq!(*center, cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0));
    assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(
        *ref_direction,
        cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
    );
    assert_eq!(*radius, 1.0);
    assert_eq!(
        result.ir.model.edges[0].param_range,
        Some([0.0, std::f64::consts::FRAC_PI_2])
    );
    assert!(result
        .ir
        .model
        .points
        .iter()
        .any(|point| point.position == cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0)));
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_solves_a_uniform_planar_curve_offset() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(uniform_offset_circle_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let offset = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D3")
        .unwrap();
    let cadmpeg_ir::geometry::CurveGeometry::Circle { radius, .. } = offset.geometry else {
        panic!("expected an exact circular offset carrier");
    };
    assert_eq!(radius, 1.5);
    let edge = result
        .ir
        .model
        .edges
        .iter()
        .find(|edge| edge.id.0 == "iges:model:edge#D3")
        .unwrap();
    assert_eq!(edge.param_range, Some([0.0, std::f64::consts::FRAC_PI_2]));
    assert_eq!(result.ir.model.procedural_curves.len(), 1);
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_solves_a_parameter_linear_line_offset() {
    for (basis_code, expected_basis) in [
        (1, cadmpeg_ir::geometry::CurveOffsetLawBasis::ArcLength),
        (2, cadmpeg_ir::geometry::CurveOffsetLawBasis::Parameter),
    ] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(linear_offset_line_file(basis_code)),
                &DecodeOptions::default(),
            )
            .unwrap();

        let offset = result
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id.0 == "iges:model:curve#D3")
            .unwrap();
        let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &offset.geometry else {
            panic!("expected an exact degree-one offset carrier");
        };
        assert_eq!(nurbs.knots, vec![0.0, 0.0, 10.0, 10.0]);
        assert_eq!(
            nurbs.control_points,
            vec![
                cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(10.0, 3.0, 0.0),
            ]
        );
        let cadmpeg_ir::geometry::ProceduralCurveDefinition::Offset {
            distance_law:
                Some(cadmpeg_ir::geometry::CurveOffsetDistanceLaw::Linear {
                    basis,
                    distances,
                    control_range,
                }),
            ..
        } = &result.ir.model.procedural_curves[0].definition
        else {
            panic!("expected a retained linear offset law");
        };
        assert_eq!(*basis, expected_basis);
        assert_eq!(*distances, [1.0, 3.0]);
        assert_eq!(*control_range, [0.0, 10.0]);
        assert!(result.report.losses.is_empty());
        let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_solves_a_polynomial_coordinate_function_offset() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(function_offset_line_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let offset = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0 == "iges:model:curve#D5")
        .unwrap();
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &offset.geometry else {
        panic!("expected an exact function-offset carrier");
    };
    assert_eq!(nurbs.knots, vec![0.0, 0.0, 10.0, 10.0]);
    assert_eq!(
        nurbs.control_points,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 3.0, 0.0),
        ]
    );
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Offset {
        distance_law:
            Some(cadmpeg_ir::geometry::CurveOffsetDistanceLaw::Coordinate {
                function,
                coordinate,
                basis,
                function_parameter_offset,
                function_parameter_scale,
            }),
        ..
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("expected a retained coordinate-function offset law");
    };
    assert_eq!(function.0, "iges:model:curve#D3");
    assert_eq!(*coordinate, 2);
    assert_eq!(*basis, cadmpeg_ir::geometry::CurveOffsetLawBasis::Parameter);
    assert_eq!(*function_parameter_offset, 0.0);
    assert_eq!(*function_parameter_scale, 0.1);
    assert!(
        result.report.losses.is_empty(),
        "{:#?}",
        result.report.losses
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_projects_a_line_as_a_normalized_bounded_wire_edge() {
    let result = IgesCodec
        .decode(&mut Cursor::new(line_file(0)), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.curves.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.points.len(), 2);
    let cadmpeg_ir::geometry::CurveGeometry::Line { origin, direction } =
        &result.ir.model.curves[0].geometry
    else {
        panic!("expected a line carrier");
    };
    assert_eq!(*origin, cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0));
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(0.6, 0.8, 0.0));
    assert_eq!(result.ir.model.edges[0].param_range, Some([0.0, 5.0]));
    assert_eq!(result.ir.model.shells[0].wire_edges.len(), 1);
    assert!(result.ir.model.shells[0].free_vertices.is_empty());
    assert_eq!(
        result.ir.model.curves[0]
            .source_object
            .as_ref()
            .unwrap()
            .object_id,
        "D1"
    );
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_preserves_semi_bounded_and_unbounded_line_domains_natively() {
    for form in [1, 2] {
        let result = IgesCodec
            .decode(&mut Cursor::new(line_file(form)), &DecodeOptions::default())
            .unwrap();

        assert_eq!(result.ir.model.curves.len(), 1);
        assert!(result.ir.model.edges.is_empty());
        assert!(result.ir.model.bodies.is_empty());
        assert_eq!(
            result.ir.model.curves[0]
                .source_object
                .as_ref()
                .unwrap()
                .object_id,
            "D1"
        );
        assert!(result.report.losses.is_empty());
        let native = result.ir.native.namespace("iges").unwrap();
        assert_eq!(native.arenas["entities"][0].fields["form"], form);
        let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

fn nested_transformed_point_file() -> Vec<u8> {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,0.5,10,2HCM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    for (sequence, parameter_start, transform, entity_type, form, label) in [
        (1, 1, 0, "124", 0, "PARENT"),
        (3, 2, 1, "124", 1, "LOCAL"),
        (5, 3, 3, "116", 0, "POINT"),
    ] {
        bytes.extend(directory_card(
            [
                entity_type,
                &parameter_start.to_string(),
                "0",
                "0",
                "0",
                "0",
                &transform.to_string(),
                "0",
                "00000000",
            ],
            sequence,
        ));
        bytes.extend(directory_card(
            [
                entity_type,
                "0",
                "0",
                "1",
                &form.to_string(),
                "",
                "",
                label,
                "0",
            ],
            sequence + 1,
        ));
    }
    bytes.extend(parameter_card(b"124,1,0,0,0,0,1,0,2,0,0,1,0;", 1, 1));
    bytes.extend(parameter_card(b"124,-1,0,0,1,0,1,0,0,0,0,1,0;", 3, 2));
    bytes.extend(parameter_card(b"116,1,2,3;", 5, 3));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000006P0000003").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

#[test]
fn decode_applies_nested_transforms_reflection_units_and_model_scale_once() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(nested_transformed_point_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.points[0].position.x, 0.0);
    assert_eq!(result.ir.model.points[0].position.y, 80.0);
    assert_eq!(result.ir.model.points[0].position.z, 60.0);
    assert_eq!(
        result.ir.native.namespace("iges").unwrap().arenas["transformations"].len(),
        2
    );
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn inspect_rejects_terminate_count_mismatch() {
    let mut bytes = card(b"original fixture", b'S', 1);
    bytes.extend(card(b"1H,,1H;,,;", b'G', 1));
    bytes.extend(card(b"S0000001G0000002D0000000P0000000", b'T', 1));

    let error = IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_ir::decode::InspectOptions::default(),
        )
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "malformed container: IGES Terminate count for global is 2, actual 1"
    );
}

#[test]
fn inspect_accepts_space_padded_terminate_counts() {
    let mut bytes = card(b"original fixture", b'S', 1);
    bytes.extend(card(b"1H,,1H;,,;", b'G', 1));
    bytes.extend(card(b"S      1G      1D      0P      0", b'T', 1));

    IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_ir::decode::InspectOptions::default(),
        )
        .unwrap();
}

#[test]
fn decode_preserves_native_entities_and_graph() {
    let bytes = point_file();

    let result = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir.source.as_ref().unwrap().format, "iges");
    let native = result.ir.native.namespace("iges").unwrap();
    assert_eq!(native.version, 2);
    assert_eq!(native.arenas["cards"].len(), 7);
    assert_eq!(native.arenas["entities"].len(), 1);
    assert!(native.arenas["colors"].is_empty());
    assert_eq!(native.arenas["display_attributes"].len(), 1);
    assert!(!native.arenas.contains_key("opaque_bytes"));
    assert_eq!(native.arenas["entities"][0].id, "iges:entity:directory#1");
    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.points[0].position.x, 1.0);
    assert_eq!(result.ir.model.points[0].position.y, 2.0);
    assert_eq!(result.ir.model.points[0].position.z, 3.0);
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert!(result.report.geometry_transferred);
    assert!(!result.report.losses.iter().any(|loss| {
        loss.message == "IGES entity type 116 form 0 retained without neutral projection"
    }));
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn inspect_preserves_transform_cycles_as_named_reference_states() {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["124", "1", "0", "0", "0", "0", "3", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        ["124", "0", "0", "1", "0", "", "", "XFORM", "1"],
        2,
    ));
    bytes.extend(directory_card(
        ["124", "2", "0", "0", "0", "0", "1", "0", "00000000"],
        3,
    ));
    bytes.extend(directory_card(
        ["124", "0", "0", "1", "0", "", "", "XFORM", "2"],
        4,
    ));
    let matrix = b"124,1.,0.,0.,0.,1.,0.,0.,0.,1.,0.,0.,0.;";
    bytes.extend(parameter_card(matrix, 1, 1));
    bytes.extend(parameter_card(matrix, 3, 2));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000004P0000002").as_bytes(),
        b'T',
        1,
    ));

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_ir::decode::InspectOptions::default(),
        )
        .unwrap();

    assert!(summary.notes.contains(&"references.cyclic=2".into()));
}

#[test]
fn compressed_and_binary_representations_are_detected_inspected_and_refused() {
    let mut compressed = vec![b' '; 80];
    compressed[72] = b'C';
    compressed.push(b'\n');
    compressed.extend(card(b"compressed fixture", b'S', 1));
    assert_eq!(IgesCodec.detect(&compressed), Confidence::High);
    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(compressed.clone()),
            &cadmpeg_ir::decode::InspectOptions::default(),
        )
        .unwrap();
    assert_eq!(summary.container_kind, "compressed-ascii");
    assert_eq!(
        IgesCodec
            .decode(&mut Cursor::new(compressed), &DecodeOptions::default())
            .unwrap_err()
            .to_string(),
        "not implemented yet: IGES Compressed ASCII representation decode"
    );

    let mut binary = vec![0_u8; 80];
    binary[0] = b'B';
    binary[1..5].copy_from_slice(&75_u32.to_be_bytes());
    binary[72] = b'B';
    binary[79] = b'1';
    assert_eq!(IgesCodec.detect(&binary), Confidence::High);
    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(binary.clone()),
            &cadmpeg_ir::decode::InspectOptions::default(),
        )
        .unwrap();
    assert_eq!(summary.container_kind, "binary");
    assert_eq!(
        IgesCodec
            .decode(&mut Cursor::new(binary), &DecodeOptions::default())
            .unwrap_err()
            .to_string(),
        "not implemented yet: IGES Binary representation decode"
    );
}

#[test]
fn legacy_fixed_ascii_is_reported_but_not_decoded_as_iges_5_3() {
    let mut bytes = point_file();
    let version = bytes
        .windows(b",11,0,".len())
        .position(|window| window == b",11,0,")
        .unwrap();
    bytes[version + 1..version + 3].copy_from_slice(b"10");

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes.clone()),
            &cadmpeg_ir::decode::InspectOptions::default(),
        )
        .unwrap();
    assert!(summary.notes.contains(&"iges_version=5.2".into()));
    assert_eq!(
        IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap_err()
            .to_string(),
        "not implemented yet: IGES Fixed ASCII version 5.2 decode; target envelope is 5.3"
    );
}

#[test]
fn decode_retains_post_terminate_physical_record() {
    let mut bytes = point_file();
    bytes.extend_from_slice(b"transport padding\r\n");

    let result = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(
        result.ir.native.namespace("iges").unwrap().arenas["cards"].len(),
        8
    );
}

/// Golden-snapshot harness pinning the read-only IGES codec before refactoring.
///
/// The fixture set is assembled from the crate's inline builders and covers the
/// `entities/` subtree broadly. Each fixture is committed three ways under
/// `tests/golden/`: the exact source bytes as `fixtures/<name>.igs`, the decode
/// output (`{ir, report, source_fidelity}`) as `decode/<name>.json`, and the
/// container inspection (`ContainerSummary`) as `inspect/<name>.json`. Goldens
/// are regenerated from the committed `.igs` bytes, so they stay reproducible
/// without the builders. There are no encode goldens: the codec is read-only.
///
/// Regenerate with `UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-iges golden`.
mod golden {
    use std::io::Cursor;
    use std::path::{Path, PathBuf};

    use cadmpeg_ir::decode::InspectOptions;

    use super::*;

    /// Build the covering fixture set: `(golden name, full IGES bytes)`. Every
    /// entry reuses an existing inline builder with the same arguments its
    /// originating white-box test passes, so the bytes exercise the real decode
    /// path. Fixtures are grouped by the `entities/` module they principally
    /// exercise; the module comments are the coverage ledger.
    fn fixtures() -> Vec<(&'static str, Vec<u8>)> {
        let mut f: Vec<(&'static str, Vec<u8>)> = Vec::new();

        // entities/geometry.rs: point, direction, line, transform, NURBS curve.
        f.push(("point", point_file()));
        f.push(("direction", direction_file()));
        f.push(("line", line_file(0)));
        f.push(("weighted_line", weighted_line_file()));
        f.push(("nurbs_curve", rational_nurbs_curve_file()));
        f.push(("nested_transformed_point", nested_transformed_point_file()));

        // entities/conics.rs: the three standard conic-arc families.
        f.push((
            "conic_ellipse",
            conic_arc_file(0, b"104,0.25,0,1,0,0,-1,0,2,0,0,1;"),
        ));
        f.push((
            "conic_hyperbola",
            conic_arc_file(
                2,
                b"104,0.25,0,-0.1111111111111111,0,0,-1,0,2,0,3.086161269630487,3.525603580931404;",
            ),
        ));
        f.push((
            "conic_parabola",
            conic_arc_file(3, b"104,1,0,0,0,-4,0,0,2,1,-2,1;"),
        ));

        // entities/composite.rs: composite curve, and one mixing analytic children.
        f.push(("composite_curve", composite_curve_file()));
        f.push((
            "mixed_analytic_composite_curve",
            mixed_analytic_composite_curve_file(),
        ));

        // entities/copious.rs: linear path, point/vector cloud, presentation witness.
        f.push((
            "copious_linear_path",
            copious_data_file(12, b"106,2,3,0,0,0,1,0,0,1,2,0;", "00000000"),
        ));
        f.push((
            "copious_points_vectors",
            copious_data_file(3, b"106,3,2,1,2,3,0,0,1,4,5,6,1,0,0;", "00000000"),
        ));
        f.push((
            "copious_witness_line",
            copious_data_file(40, b"106,1,3,0,0,0,1,0,2,0;", "00000100"),
        ));

        // entities/splines.rs: parametric spline curve and surface.
        f.push(("parametric_spline_curve", parametric_spline_curve_file()));
        f.push((
            "parametric_spline_surface",
            parametric_spline_surface_file(),
        ));

        // entities/surfaces.rs: plane, ruled, tabulated cylinder, revolution, NURBS.
        f.push(("plane", plane_file()));
        f.push(("ruled_surface", ruled_surface_file()));
        f.push(("tabulated_cylinder", tabulated_cylinder_file()));
        f.push(("surface_of_revolution", surface_of_revolution_file()));
        f.push((
            "placed_surface_of_revolution",
            placed_surface_of_revolution_file(),
        ));
        f.push(("nurbs_surface", nurbs_surface_file()));

        // entities/analytic_surfaces.rs: the five pointer-defined analytic forms.
        f.push((
            "analytic_plane_surface",
            pointer_defined_surface_file(190, 1),
        ));
        f.push((
            "analytic_cylinder_surface",
            pointer_defined_surface_file(192, 0),
        ));
        f.push((
            "analytic_cone_surface",
            pointer_defined_surface_file(194, 1),
        ));
        f.push((
            "analytic_sphere_surface",
            pointer_defined_surface_file(196, 1),
        ));
        f.push((
            "analytic_torus_surface",
            pointer_defined_surface_file(198, 1),
        ));

        // entities/offsets.rs: offset curves (uniform, linear, function) and surface.
        f.push(("uniform_offset_circle", uniform_offset_circle_file()));
        f.push(("linear_offset_line", linear_offset_line_file(1)));
        f.push(("function_offset_line", function_offset_line_file()));
        f.push(("offset_plane", offset_plane_file(1.0, 2.0)));

        // entities/trimming.rs: trimmed sheets, bounded planes, boundary loops.
        f.push(("trimmed_plane", trimmed_plane_file()));
        f.push((
            "trimmed_plane_inner_loop",
            trimmed_plane_with_inner_loop_file(),
        ));
        f.push((
            "parameter_domain_trimmed_surface",
            parameter_domain_trimmed_surface_file(),
        ));
        f.push(("bounded_plane", bounded_plane_file()));
        f.push((
            "parametrically_bounded_plane",
            parametrically_bounded_plane_file(),
        ));
        f.push(("multi_pcurve_boundary", multi_pcurve_boundary_file()));

        // entities/brep.rs: shells, manifold/void solids, vertex/pcurve loops.
        f.push(("explicit_open_shell", explicit_open_shell_file()));
        f.push((
            "explicit_non_manifold_open_shell",
            explicit_non_manifold_open_shell_file(),
        ));
        f.push((
            "explicit_tetrahedron_solid",
            explicit_tetrahedron_solid_file(),
        ));
        f.push((
            "explicit_tetrahedron_solid_with_boolean",
            explicit_tetrahedron_solid_with_boolean_file(),
        ));
        f.push(("explicit_void_solid", explicit_void_solid_file().0));
        f.push(("explicit_vertex_loop", explicit_vertex_loop_file()));
        f.push((
            "colored_explicit_vertex_loop",
            colored_explicit_vertex_loop_file(),
        ));
        f.push(("explicit_cylinder_seam", explicit_cylinder_seam_file()));
        f.push((
            "explicit_multi_pcurve_loop",
            explicit_multi_pcurve_loop_file(),
        ));

        // entities/csg.rs: primitives, procedural/boolean trees, assemblies, instances.
        f.push(("primitive_solids", primitive_solids_file()));
        f.push((
            "procedural_and_boolean_solids",
            procedural_and_boolean_solids_file(),
        ));
        f.push(("solid_assembly", solid_assembly_file()));
        f.push(("solid_instance", solid_instance_file()));
        f.push(("patterned_instance", patterned_instance_file()));

        // entities/structure.rs: references, groups, attributes, properties,
        // subfigures, networks, units, levels, associativities. The null entity
        // (type 0) routes through the same `structure::project` retention path.
        f.push((
            "null_entity",
            owned_test_file(&[OwnedTestEntity {
                entity_type: 0,
                form: 0,
                label: "NULL".into(),
                status: "00000000",
                parameters: "0;".into(),
            }]),
        ));
        f.push(("external_reference_forms", external_reference_forms_file()));
        f.push(("group_forms", group_forms_file()));
        f.push((
            "attribute_definition_forms",
            attribute_definition_forms_file(),
        ));
        f.push(("attribute_instance_forms", attribute_instance_forms_file()));
        f.push(("product_property", product_property_file()));
        f.push((
            "variable_schema_property_forms",
            variable_schema_property_forms_file(),
        ));
        f.push(("scalar_property_forms", scalar_property_forms_file()));
        f.push(("nested_subfigure", nested_subfigure_file()));
        f.push(("network_subfigure", network_subfigure_file()));
        f.push((
            "connected_network_subfigure",
            connected_network_subfigure_file(),
        ));
        f.push(("units_data", units_data_file()));
        f.push(("definition_levels", definition_levels_file()));
        f.push(("associativity_definition", associativity_definition_file()));
        f.push((
            "bounded_associativity_forms",
            bounded_associativity_forms_file(),
        ));
        f.push(("flow_associativity_forms", flow_associativity_forms_file()));
        f.push(("view_visibility_forms", view_visibility_forms_file()));

        // entities/drawing.rs: views and drawings carrying properties.
        f.push(("view_forms", view_forms_file()));
        f.push(("drawing_with_properties", drawing_with_properties_file()));

        // entities/presentation.rs: line-font and text-font definitions.
        f.push(("line_font_definitions", line_font_definitions_file()));
        f.push(("text_font_definition", text_font_definition_file()));

        // entities/annotation.rs: text, leaders, dimensions, labels, symbols.
        f.push(("text_annotation", text_annotation_file()));
        f.push(("leader_forms", leader_forms_file()));
        f.push(("dimension_forms", dimension_forms_file()));
        f.push((
            "legacy_dimension_and_label_forms",
            legacy_dimension_and_label_forms_file(),
        ));
        f.push((
            "symbol_and_sectioned_area",
            symbol_and_sectioned_area_file(),
        ));
        f.push((
            "text_display_template_forms",
            text_display_template_forms_file(),
        ));

        f
    }

    /// Serialize the decode output for one fixture as stable pretty JSON:
    /// canonical IR, decode report, and source-fidelity sidecar. A decode error
    /// is frozen too (an input the codec refuses is contract-relevant behavior),
    /// so this never panics on codec output.
    fn decode_snapshot(bytes: &[u8]) -> String {
        let value =
            match IgesCodec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default()) {
                Ok(result) => serde_json::json!({
                    "ir": serde_json::to_value(&result.ir).expect("serialize ir"),
                    "report": serde_json::to_value(&result.report).expect("serialize report"),
                    "source_fidelity": serde_json::to_value(&result.source_fidelity)
                        .expect("serialize source_fidelity"),
                }),
                Err(err) => serde_json::json!({ "decode_error": err.to_string() }),
            };
        let mut text = serde_json::to_string_pretty(&value).expect("serialize decode snapshot");
        text.push('\n');
        text
    }

    /// Serialize the container inspection for one fixture as stable pretty JSON.
    /// Inspection errors are frozen for the same reason decode errors are.
    fn inspect_snapshot(bytes: &[u8]) -> String {
        let value =
            match IgesCodec.inspect(&mut Cursor::new(bytes.to_vec()), &InspectOptions::default()) {
                Ok(summary) => serde_json::to_value(&summary).expect("serialize inspect"),
                Err(err) => serde_json::json!({ "inspect_error": err.to_string() }),
            };
        let mut text = serde_json::to_string_pretty(&value).expect("serialize inspect snapshot");
        text.push('\n');
        text
    }

    fn golden_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
    }

    fn fixture_path(name: &str) -> PathBuf {
        golden_dir().join("fixtures").join(format!("{name}.igs"))
    }

    fn decode_path(name: &str) -> PathBuf {
        golden_dir().join("decode").join(format!("{name}.json"))
    }

    fn inspect_path(name: &str) -> PathBuf {
        golden_dir().join("inspect").join(format!("{name}.json"))
    }

    /// First line that differs between two documents, 1-based, with both sides
    /// truncated for a readable failure.
    fn first_line_diff(expected: &str, actual: &str) -> (usize, String, String) {
        let mut exp = expected.lines();
        let mut act = actual.lines();
        let mut line = 0usize;
        loop {
            line += 1;
            match (exp.next(), act.next()) {
                (Some(e), Some(a)) if e == a => {}
                (e, a) => {
                    let trunc = |s: Option<&str>| match s {
                        Some(s) if s.len() > 200 => format!("{}…", &s[..200]),
                        Some(s) => s.to_string(),
                        None => "<end of file>".to_string(),
                    };
                    return (line, trunc(e), trunc(a));
                }
            }
        }
    }

    fn update_requested() -> bool {
        std::env::var_os("UPDATE_GOLDEN").is_some()
    }

    /// Compare one regenerated snapshot against its committed golden, pushing a
    /// readable failure when they diverge or the golden cannot be read.
    fn compare(path: &Path, actual: &str, name: &str, kind: &str, failures: &mut Vec<String>) {
        let expected = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) => {
                failures.push(format!(
                    "fixture `{name}` ({kind}): cannot read golden {} ({e}); run `UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-iges golden`",
                    path.display()
                ));
                return;
            }
        };
        if expected != actual {
            let (line, exp_line, act_line) = first_line_diff(&expected, actual);
            failures.push(format!(
                "fixture `{name}` ({kind}): output diverged from golden at line {line}\n    golden: {exp_line}\n    actual: {act_line}"
            ));
        }
    }

    /// Freezes the codec's decode and inspect output against the committed
    /// `.igs` bytes. Under `UPDATE_GOLDEN`, (re)writes the source bytes and both
    /// snapshots; otherwise regenerates snapshots from the committed bytes and
    /// asserts byte-identity, and separately asserts the committed bytes still
    /// reproduce from their builder.
    #[test]
    fn golden_snapshots_are_byte_identical() {
        let update = update_requested();
        if update {
            for sub in ["fixtures", "decode", "inspect"] {
                std::fs::create_dir_all(golden_dir().join(sub)).expect("create golden dir");
            }
        }
        let mut failures: Vec<String> = Vec::new();
        for (name, bytes) in fixtures() {
            if update {
                std::fs::write(fixture_path(name), &bytes)
                    .unwrap_or_else(|e| panic!("write fixture {name}: {e}"));
                std::fs::write(decode_path(name), decode_snapshot(&bytes).as_bytes())
                    .unwrap_or_else(|e| panic!("write decode golden {name}: {e}"));
                std::fs::write(inspect_path(name), inspect_snapshot(&bytes).as_bytes())
                    .unwrap_or_else(|e| panic!("write inspect golden {name}: {e}"));
                continue;
            }
            let committed = match std::fs::read(fixture_path(name)) {
                Ok(bytes) => bytes,
                Err(e) => {
                    failures.push(format!(
                        "fixture `{name}`: cannot read committed {} ({e}); run `UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-iges golden`",
                        fixture_path(name).display()
                    ));
                    continue;
                }
            };
            if committed != bytes {
                failures.push(format!(
                    "fixture `{name}`: committed .igs bytes no longer reproduce from the builder; run `UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-iges golden`"
                ));
            }
            compare(
                &decode_path(name),
                &decode_snapshot(&committed),
                name,
                "decode",
                &mut failures,
            );
            compare(
                &inspect_path(name),
                &inspect_snapshot(&committed),
                name,
                "inspect",
                &mut failures,
            );
        }
        assert!(
            failures.is_empty(),
            "{} golden snapshot(s) drifted; if the change is intended run `UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-iges golden` and review the diff:\n\n{}",
            failures.len(),
            failures.join("\n\n")
        );
    }

    /// Guards against nondeterministic codec output (`HashMap` iteration order,
    /// timestamps): decoding and inspecting the same bytes twice must produce
    /// identical JSON.
    #[test]
    fn golden_output_is_deterministic() {
        for (name, bytes) in fixtures() {
            let (first, second) = (decode_snapshot(&bytes), decode_snapshot(&bytes));
            if first != second {
                let (line, a, b) = first_line_diff(&first, &second);
                panic!("fixture `{name}`: nondeterministic decode at line {line}\n    run 1: {a}\n    run 2: {b}");
            }
            let (first, second) = (inspect_snapshot(&bytes), inspect_snapshot(&bytes));
            if first != second {
                let (line, a, b) = first_line_diff(&first, &second);
                panic!("fixture `{name}`: nondeterministic inspect at line {line}\n    run 1: {a}\n    run 2: {b}");
            }
        }
    }
}
