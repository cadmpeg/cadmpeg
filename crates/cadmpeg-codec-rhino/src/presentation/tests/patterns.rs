// SPDX-License-Identifier: Apache-2.0

use cadmpeg_test_support::EditableDecodeResult;

use crate::chunks::ArchiveVersion;
use crate::loss::RhinoLossCode;
use crate::presentation::install;
use crate::presentation::parse_hatch_pattern;
use crate::presentation::parse_linetype;
use crate::presentation::tests::anonymous;
use crate::presentation::tests::anonymous_body;
use crate::presentation::LinetypeRecord;
use crate::presentation::PatternTransferError;
use crate::presentation::ANONYMOUS;
use crate::presentation::HATCH_PATTERN;
use crate::presentation::HATCH_PATTERN_TABLE;
use crate::presentation::LINETYPE;
use crate::presentation::LINETYPE_TABLE;
use crate::presentation::MODEL_ATTRIBUTES;
use crate::settings::StandardUnit;
use crate::settings::UnitBinding;
use crate::test_support::test_dump::utf16_bytes;
use crate::wire::Uuid;
use cadmpeg_ir::document::CadIr;

fn modern_linetype_record(archive: ArchiveVersion, always_model_distance: bool) -> Vec<u8> {
    let mut component = 1_i32.to_le_bytes().to_vec();
    component.extend(0_i32.to_le_bytes());
    component.push(0);
    component.push(1);
    component.extend([0x33; 16]);
    component.push(0);
    component.push(1);
    component.extend(9_i32.to_le_bytes());
    component.push(1);
    component.extend(utf16_bytes("modern dash"));
    component.extend(crc32fast::hash(&component).to_le_bytes());
    let mut attributes = MODEL_ATTRIBUTES.to_le_bytes().to_vec();
    attributes.extend((component.len() as i64).to_le_bytes());
    attributes.extend(component);

    let mut body = attributes;
    body.extend(2_i32.to_le_bytes());
    body.extend(2.5_f64.to_le_bytes());
    body.extend(0_u32.to_le_bytes());
    body.extend(1.25_f64.to_le_bytes());
    body.extend(1_u32.to_le_bytes());
    body.extend([1, 1, 2, 2]);
    body.push(3);
    body.extend(2.75_f64.to_le_bytes());
    body.extend([4, 2]);
    body.push(5);
    body.extend(3_i32.to_le_bytes());
    for value in [[0.0_f64, 0.5], [0.35_f64, 1.25], [1.0_f64, 2.5]] {
        body.extend(value[0].to_le_bytes());
        body.extend(value[1].to_le_bytes());
    }
    if always_model_distance {
        body.extend([6, 1]);
    }
    body.push(0);

    let mut payload = 2_i32.to_le_bytes().to_vec();
    payload.extend(3_i32.to_le_bytes());
    payload.extend(body);
    let anonymous = crate::test_support::test_dump::crc_chunk(archive, ANONYMOUS, &payload);
    {
        let class =
            crate::test_support::test_dump::class_wrapper(archive, LINETYPE.to_wire(), &anonymous);
        crate::test_support::test_dump::crc_chunk_excluding(
            archive,
            0x2000_8078,
            &class,
            std::slice::from_ref(&(0..class.len())),
        )
    }
}

fn modern_hatch_pattern_record(
    archive: ArchiveVersion,
    pattern_unit_system: u8,
    always_model_distances: bool,
) -> Vec<u8> {
    let mut line = 0.5_f64.to_le_bytes().to_vec();
    for value in [1.0_f64, 2.0, 3.0, 4.0] {
        line.extend(value.to_le_bytes());
    }
    line.extend(2_i32.to_le_bytes());
    line.extend(5.0_f64.to_le_bytes());
    line.extend((-2.0_f64).to_le_bytes());
    let mut line_list = 1_i32.to_le_bytes().to_vec();
    line_list.extend(anonymous(0, &line));

    let mut component = 1_i32.to_le_bytes().to_vec();
    component.extend(0_i32.to_le_bytes());
    component.push(0);
    component.push(1);
    component.extend([0x22; 16]);
    component.push(0);
    component.push(1);
    component.extend(5_i32.to_le_bytes());
    component.push(1);
    component.extend(utf16_bytes("modern hatch"));
    component.extend(crc32fast::hash(&component).to_le_bytes());
    let mut component_chunk = MODEL_ATTRIBUTES.to_le_bytes().to_vec();
    component_chunk.extend((component.len() as i64).to_le_bytes());
    component_chunk.extend(component);

    let mut body = component_chunk;
    body.extend(1_i32.to_le_bytes());
    body.extend(utf16_bytes("modern description"));
    body.extend(anonymous_body(&line_list));
    body.push(pattern_unit_system);
    body.push(u8::from(always_model_distances));
    let anonymous = anonymous(0, &body);
    let class =
        crate::test_support::test_dump::class_wrapper(archive, HATCH_PATTERN.to_wire(), &anonymous);
    crate::test_support::test_dump::crc_chunk_excluding(
        archive,
        0x2000_8077,
        &class,
        std::slice::from_ref(&(0..class.len())),
    )
}

fn legacy_hatch_pattern_record(archive: ArchiveVersion) -> Vec<u8> {
    let mut payload = vec![0x12];
    payload.extend(3_i32.to_le_bytes());
    payload.extend(1_i32.to_le_bytes());
    payload.extend(utf16_bytes("cross"));
    payload.extend(utf16_bytes("cross hatch"));
    payload.extend(1_i32.to_le_bytes());
    payload.push(0x11);
    payload.extend(0.5_f64.to_le_bytes());
    for value in [1.0_f64, 2.0, 3.0, 4.0] {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(2_i32.to_le_bytes());
    payload.extend(5.0_f64.to_le_bytes());
    payload.extend((-2.0_f64).to_le_bytes());
    payload.extend([0x77; 16]);
    {
        let class = crate::test_support::test_dump::class_wrapper(
            archive,
            HATCH_PATTERN.to_wire(),
            &payload,
        );
        crate::test_support::test_dump::crc_chunk_excluding(
            archive,
            0x2000_8077,
            &class,
            std::slice::from_ref(&(0..class.len())),
        )
    }
}

#[test]
fn absent_component_index_does_not_alias_system_index_minus_one() {
    let record = LinetypeRecord {
        id: "rhino:presentation:linetype#record-7".to_owned(),
        source_offset: 7,
        archive_index: None,
        source_uuid: None,
        name: String::new(),
        segments: Vec::new(),
        line_cap: 0,
        line_join: 0,
        width: crate::test_support::finite(0.0),
        width_units: 0,
        taper_points: Vec::new(),
        always_model_distance: false,
    };
    assert_eq!(record.archive_index, None);
    let json = serde_json::to_value(record).expect("linetype record JSON");
    assert!(json.get("archive_index").is_none());
}

#[test]
fn legacy_linetype_preserves_print_lengths_and_wire_segment_tags() {
    let mut body = 4_i32.to_le_bytes().to_vec();
    body.extend(utf16_bytes("dash"));
    body.extend(2_i32.to_le_bytes());
    body.extend(2.0_f64.to_le_bytes());
    body.extend(0_u32.to_le_bytes());
    body.extend(1.0_f64.to_le_bytes());
    body.extend(1_u32.to_le_bytes());
    body.extend([0x66; 16]);
    body.extend([0xaa, 0xbb]);
    let bytes = anonymous(15, &body);
    let value = parse_linetype(
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V5,
        UnitBinding::Millimeters(StandardUnit::Centimeters.into()),
        0,
    )
    .expect("required invariant");
    assert_eq!(value.name, "dash");
    assert_eq!(
        value.segments[0].length_millimeters,
        crate::test_support::finite(2.0)
    );
    assert_eq!(value.segments[0].segment_type, 0);
    assert_eq!(
        value.segments[1].length_millimeters,
        crate::test_support::finite(1.0)
    );
    assert_eq!(value.segments[1].segment_type, 1);
}

#[test]
fn modern_linetype_scales_only_model_distance_segments() {
    fn modern_linetype(always_model_distance: bool) -> Vec<u8> {
        let mut component = 1_i32.to_le_bytes().to_vec();
        component.extend(0_i32.to_le_bytes());
        component.push(0);
        component.push(1);
        component.extend([0x33; 16]);
        component.push(0);
        component.push(1);
        component.extend(9_i32.to_le_bytes());
        component.push(1);
        component.extend(utf16_bytes("modern dash"));
        component.extend(crc32fast::hash(&component).to_le_bytes());
        let mut attributes = MODEL_ATTRIBUTES.to_le_bytes().to_vec();
        attributes.extend((component.len() as i64).to_le_bytes());
        attributes.extend(component);

        let mut body = attributes;
        body.extend(2_i32.to_le_bytes());
        body.extend(2.5_f64.to_le_bytes());
        body.extend(0_u32.to_le_bytes());
        body.extend(1.25_f64.to_le_bytes());
        body.extend(1_u32.to_le_bytes());
        body.extend([1, 1, 2, 2]);
        body.push(3);
        body.extend(2.75_f64.to_le_bytes());
        body.extend([4, 2]);
        body.push(5);
        body.extend(3_i32.to_le_bytes());
        for value in [[0.0_f64, 0.5], [0.35_f64, 1.25], [1.0_f64, 2.5]] {
            body.extend(value[0].to_le_bytes());
            body.extend(value[1].to_le_bytes());
        }
        if always_model_distance {
            body.extend([6, 1]);
        }
        body.push(0);

        let mut payload = 2_i32.to_le_bytes().to_vec();
        payload.extend(3_i32.to_le_bytes());
        payload.extend(body);
        payload.extend(crc32fast::hash(&payload).to_le_bytes());
        let mut bytes = ANONYMOUS.to_le_bytes().to_vec();
        bytes.extend((payload.len() as i64).to_le_bytes());
        bytes.extend(payload);
        bytes
    }

    let model_distance_bytes = modern_linetype(true);
    let model_distance = parse_linetype(
        &model_distance_bytes,
        0..model_distance_bytes.len(),
        ArchiveVersion::V8,
        UnitBinding::Millimeters(StandardUnit::Inches.into()),
        0,
    )
    .expect("model-distance linetype");
    assert_eq!(model_distance.name, "modern dash");
    assert_eq!(model_distance.archive_index, Some(9));
    assert_eq!(
        model_distance.segments[0].length_millimeters,
        crate::test_support::finite(63.5)
    );
    assert_eq!(model_distance.segments[0].segment_type, 0);
    assert_eq!(
        model_distance.segments[1].length_millimeters,
        crate::test_support::finite(31.75)
    );
    assert_eq!(model_distance.segments[1].segment_type, 1);
    assert_eq!(model_distance.line_cap, 1);
    assert_eq!(model_distance.line_join, 2);
    assert_eq!(model_distance.width, crate::test_support::finite(2.75));
    assert_eq!(model_distance.width_units, 2);
    assert_eq!(
        model_distance.taper_points,
        vec![[0.0, 0.5], [0.35, 1.25], [1.0, 2.5]]
            .into_iter()
            .map(crate::test_support::finite_array)
            .collect::<Vec<_>>()
    );
    assert!(model_distance.always_model_distance);

    let unbound_model_distance = parse_linetype(
        &model_distance_bytes,
        0..model_distance_bytes.len(),
        ArchiveVersion::V8,
        UnitBinding::Unavailable,
        0,
    );
    assert!(matches!(
        unbound_model_distance,
        Err(PatternTransferError::UnavailableDocumentUnits)
    ));

    let print_distance_bytes = modern_linetype(false);
    let print_distance = parse_linetype(
        &print_distance_bytes,
        0..print_distance_bytes.len(),
        ArchiveVersion::V8,
        UnitBinding::Millimeters(StandardUnit::Inches.into()),
        0,
    )
    .expect("print-distance linetype");
    assert_eq!(
        print_distance.segments[0].length_millimeters,
        crate::test_support::finite(2.5)
    );
    assert_eq!(
        print_distance.segments[1].length_millimeters,
        crate::test_support::finite(1.25)
    );
    assert!(!print_distance.always_model_distance);
    assert_eq!(print_distance.taper_points, model_distance.taper_points);
}

#[test]
fn linetype_install_retains_model_distances_without_physical_units() {
    for (unit, always_model_distance, typed) in [
        (Some(2), true, true),
        (Some(0), true, false),
        (Some(255), true, false),
        (None, true, false),
        (Some(0), false, true),
        (None, false, true),
    ] {
        let archive = ArchiveVersion::V8;
        let record = modern_linetype_record(archive, always_model_distance);
        let unit_records = unit
            .map(|value| vec![crate::test_support::test_dump::units_record(archive, value)])
            .unwrap_or_default();
        let bytes = crate::test_support::test_dump::minimal_document(
            "80",
            &[
                crate::test_support::test_dump::table(archive, 0x1000_0014, &[]),
                crate::test_support::test_dump::table(archive, 0x1000_0015, &unit_records),
                crate::test_support::test_dump::table(
                    archive,
                    LINETYPE_TABLE,
                    std::slice::from_ref(&record),
                ),
                crate::test_support::test_dump::table(archive, 0x1000_0013, &[]),
            ],
        );
        let scan = crate::container::scan_owned(bytes.clone()).expect("complete linetype document");
        let mut ir = CadIr::empty();
        let installed = install(&scan, &mut ir).expect("linetype install");
        let linetypes = &ir.native.namespace("rhino").unwrap().arenas()["linetypes"];
        assert_eq!(linetypes.len(), usize::from(typed), "unit={unit:?}");
        assert_eq!(
            installed.opaque_records.len(),
            usize::from(!typed),
            "unit={unit:?}"
        );
        if !typed {
            assert!(installed
                .opaque_records
                .iter()
                .any(|source| { bytes[source.record.range.clone()] == record }));
            assert!(installed.losses.iter().any(|loss| {
                loss.code == RhinoLossCode::PresentationRecordDropped.kind()
                    && loss.message.contains("no physical millimetre binding")
            }));
        }
    }
}

#[test]
fn hatch_install_retains_document_distances_without_physical_units() {
    for unit in [Some(2), Some(0), Some(255), None] {
        let archive = ArchiveVersion::V8;
        let record = legacy_hatch_pattern_record(archive);
        let unit_records = unit
            .map(|value| crate::test_support::test_dump::units_record(archive, value))
            .into_iter()
            .collect::<Vec<_>>();
        let bytes = crate::test_support::test_dump::minimal_document(
            "80",
            &[
                crate::test_support::test_dump::table(archive, 0x1000_0014, &[]),
                crate::test_support::test_dump::table(archive, 0x1000_0015, &unit_records),
                crate::test_support::test_dump::table(
                    archive,
                    HATCH_PATTERN_TABLE,
                    std::slice::from_ref(&record),
                ),
                crate::test_support::test_dump::table(archive, 0x1000_0013, &[]),
            ],
        );
        let scan = crate::container::scan_owned(bytes.clone()).expect("complete hatch document");
        let mut ir = CadIr::empty();
        let installed = install(&scan, &mut ir).expect("hatch install");
        let hatch_patterns = &ir.native.namespace("rhino").unwrap().arenas()["hatch_patterns"];
        let typed = unit == Some(2);
        assert_eq!(hatch_patterns.len(), usize::from(typed), "unit={unit:?}");
        assert_eq!(
            installed.opaque_records.len(),
            usize::from(!typed),
            "unit={unit:?}"
        );
        if !typed {
            assert!(installed
                .opaque_records
                .iter()
                .any(|source| { bytes[source.record.range.clone()] == record }));
            assert!(installed.losses.iter().any(|loss| {
                loss.code == RhinoLossCode::PresentationRecordDropped.kind()
                    && loss.message.contains("no physical millimetre binding")
            }));
        }
    }
}

#[test]
fn modern_hatch_pattern_uses_its_explicit_unit_binding() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    for (document_unit, pattern_unit, always_model_distances, typed, expected_base) in [
        (Some(2), 8, true, true, Some([25.4, 50.8])),
        (None, 8, true, true, Some([25.4, 50.8])),
        (Some(0), 0, true, false, None),
        (None, 255, true, false, None),
        (None, 8, false, true, Some([25.4, 50.8])),
        (Some(3), 8, false, true, Some([25.4, 50.8])),
        (Some(3), 0, false, true, Some([10.0, 20.0])),
        (None, 0, false, false, None),
        (Some(2), 255, true, false, None),
        (Some(2), 255, false, false, None),
        (Some(2), 11, true, false, None),
        (Some(2), 11, false, false, None),
        (Some(2), 26, true, false, None),
    ] {
        let archive = ArchiveVersion::V9;
        let record = modern_hatch_pattern_record(archive, pattern_unit, always_model_distances);
        let unit_records = document_unit
            .map(|value| vec![crate::test_support::test_dump::units_record(archive, value)])
            .unwrap_or_default();
        let bytes = crate::test_support::test_dump::minimal_document(
            "90",
            &[
                crate::test_support::test_dump::table(archive, 0x1000_0014, &[]),
                crate::test_support::test_dump::table(archive, 0x1000_0015, &unit_records),
                crate::test_support::test_dump::table(
                    archive,
                    HATCH_PATTERN_TABLE,
                    std::slice::from_ref(&record),
                ),
                crate::test_support::test_dump::table(archive, 0x1000_0013, &[]),
            ],
        );
        let scan = crate::container::scan_owned(bytes.clone()).expect("complete hatch document");
        let mut ir = CadIr::empty();
        let installed = install(&scan, &mut ir).expect("hatch install");
        let hatch_patterns = &ir.native.namespace("rhino").unwrap().arenas()["hatch_patterns"];
        assert_eq!(
            hatch_patterns.len(),
            usize::from(typed),
            "document={document_unit:?} pattern={pattern_unit} always={always_model_distances} losses={:?}",
            installed.losses
        );
        assert_eq!(
            installed.opaque_records.len(),
            usize::from(!typed),
            "document={document_unit:?} pattern={pattern_unit} always={always_model_distances} losses={:?}",
            installed.losses
        );
        match expected_base {
            Some(expected) => {
                let value = serde_json::to_value(&hatch_patterns[0]).expect("hatch JSON");
                assert_eq!(
                    value["lines"][0]["base_millimeters"],
                    serde_json::json!(expected)
                );
            }
            None => assert!(hatch_patterns.is_empty()),
        }
        let decoded = EditableDecodeResult::from(
            crate::RhinoCodec
                .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
                .expect("complete hatch decode"),
        );
        let admitted: CadIr = serde_json::from_slice(
            &serde_json::to_vec(decoded.ir()).expect("complete hatch CADIR"),
        )
        .expect("complete hatch CADIR admission");
        let records = &admitted.native.namespace("rhino").unwrap().arenas()["hatch_patterns"];
        assert_eq!(records.len(), usize::from(typed));
        if let Some(expected) = expected_base {
            let value = serde_json::to_value(&records[0]).expect("admitted hatch JSON");
            assert_eq!(
                value["lines"][0]["base_millimeters"],
                serde_json::json!(expected)
            );
        } else {
            assert!(decoded
                .source_fidelity()
                .retained_records()
                .values()
                .any(|source| { source.data() == Some(record.as_slice()) }));
            let reason = if matches!(pattern_unit, 11 | 26 | 255) {
                format!("hatch pattern unit code {pattern_unit}")
            } else {
                "no physical millimetre binding".to_owned()
            };
            assert!(decoded.report().losses.iter().any(|loss| {
                loss.code == RhinoLossCode::PresentationRecordDropped.kind()
                    && loss.message.contains(&reason)
                    && loss.message.contains(&format!(
                        "offset {}",
                        scan.tables
                            .iter()
                            .find(|table| table.typecode == HATCH_PATTERN_TABLE)
                            .unwrap()
                            .records[0]
                            .range
                            .start
                    ))
            }));
        }
    }
}

#[test]
fn solid_hatch_pattern_needs_no_length_binding() {
    let mut bytes = vec![0x12];
    bytes.extend(3_i32.to_le_bytes());
    bytes.extend(0_i32.to_le_bytes());
    bytes.extend(utf16_bytes("solid"));
    bytes.extend(utf16_bytes("solid fill"));
    bytes.extend([0x77; 16]);
    for binding in [UnitBinding::Native, UnitBinding::Unavailable] {
        let pattern = parse_hatch_pattern(&bytes, 0..bytes.len(), ArchiveVersion::V5, binding, 23)
            .expect("solid hatch has no dimensional payload");
        assert!(pattern.lines.is_empty());
        assert_eq!(pattern.fill_type, 0);
        let mut ir = CadIr::empty();
        ir.native
            .namespace_mut("rhino")
            .set_arena("hatch_patterns", &[pattern])
            .unwrap();
        let admitted: CadIr = serde_json::from_slice(&serde_json::to_vec(&ir).unwrap()).unwrap();
        assert_eq!(
            admitted.native.namespace("rhino").unwrap().arenas()["hatch_patterns"].len(),
            1
        );
    }
}

#[test]
fn legacy_hatch_pattern_scales_line_offsets_and_dashes() {
    let mut bytes = vec![0x12];
    bytes.extend(3_i32.to_le_bytes());
    bytes.extend(1_i32.to_le_bytes());
    bytes.extend(utf16_bytes("cross"));
    bytes.extend(utf16_bytes("cross hatch"));
    bytes.extend(1_i32.to_le_bytes());
    bytes.push(0x11);
    bytes.extend(0.5_f64.to_le_bytes());
    for value in [1.0_f64, 2.0, 3.0, 4.0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(2_i32.to_le_bytes());
    bytes.extend(5.0_f64.to_le_bytes());
    bytes.extend((-2.0_f64).to_le_bytes());
    bytes.extend([0x77; 16]);
    let value = parse_hatch_pattern(
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V5,
        UnitBinding::Millimeters(StandardUnit::Centimeters.into()),
        0,
    )
    .expect("required invariant");
    assert_eq!(
        value.lines[0].base_millimeters,
        crate::test_support::finite_array([10.0, 20.0])
    );
    assert_eq!(
        value.lines[0].offset_millimeters,
        crate::test_support::finite_array([30.0, 40.0])
    );
    assert_eq!(
        value.lines[0].dashes_millimeters,
        crate::test_support::finite_array([50.0, -20.0])
    );
    assert_eq!(
        value.lines[0].angle_radians,
        crate::test_support::finite(0.5)
    );

    let unbound = parse_hatch_pattern(
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V5,
        UnitBinding::Unavailable,
        0,
    );
    assert!(matches!(
        unbound,
        Err(PatternTransferError::UnavailableDocumentUnits)
    ));
}

#[test]
fn modern_hatch_pattern_reads_nested_line_chunks() {
    let mut line = 0.375_f64.to_le_bytes().to_vec();
    for value in [1.25_f64, -2.5, 3.5, 4.75] {
        line.extend(value.to_le_bytes());
    }
    line.extend(3_i32.to_le_bytes());
    for value in [1.25_f64, -0.75, 0.5] {
        line.extend(value.to_le_bytes());
    }
    line.extend([0xa5; 3]);
    let mut line_list = 1_i32.to_le_bytes().to_vec();
    line_list.extend(anonymous(0, &line));
    line_list.extend([0xb6; 5]);

    let mut component = 1_i32.to_le_bytes().to_vec();
    component.extend(0_i32.to_le_bytes());
    component.push(0);
    component.push(1);
    component.extend([0x22; 16]);
    component.push(0);
    component.push(1);
    component.extend(5_i32.to_le_bytes());
    component.push(1);
    component.extend(utf16_bytes("modern hatch"));
    let mut component_payload = component.clone();
    component_payload.extend(crc32fast::hash(&component_payload).to_le_bytes());
    let mut component_chunk = MODEL_ATTRIBUTES.to_le_bytes().to_vec();
    component_chunk.extend((component_payload.len() as i64).to_le_bytes());
    component_chunk.extend(component_payload);

    let mut body = component_chunk;
    body.extend(1_i32.to_le_bytes());
    body.extend(utf16_bytes("modern description"));
    body.extend(anonymous_body(&line_list));
    let mut v8_body = body.clone();
    v8_body.extend([0xc7; 4]);
    let bytes = anonymous(0, &v8_body);
    let value = parse_hatch_pattern(
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V8,
        UnitBinding::Millimeters(StandardUnit::Centimeters.into()),
        321,
    )
    .expect("modern hatch pattern");

    assert_eq!(value.archive_index, Some(5));
    assert_eq!(
        value.source_uuid,
        Some(Uuid::from_canonical([0x22; 16]).to_string())
    );
    assert_eq!(value.name, "modern hatch");
    assert_eq!(value.description, "modern description");
    assert_eq!(
        value.lines[0].angle_radians,
        crate::test_support::finite(0.375)
    );
    assert_eq!(
        value.lines[0].base_millimeters,
        crate::test_support::finite_array([12.5, -25.0])
    );
    assert_eq!(
        value.lines[0].offset_millimeters,
        crate::test_support::finite_array([35.0, 47.5])
    );
    assert_eq!(
        value.lines[0].dashes_millimeters,
        crate::test_support::finite_array([12.5, -7.5, 5.0])
    );
    assert_eq!(
        value
            .distance_settings
            .map(|settings| settings.pattern_unit_system),
        None
    );
    assert_eq!(
        value
            .distance_settings
            .map(|settings| settings.always_model_distances),
        None
    );

    let mut v9_body = body;
    v9_body.extend([2, 1]);
    v9_body.extend([0xd8; 4]);
    let v9_bytes = anonymous(0, &v9_body);
    let v9 = parse_hatch_pattern(
        &v9_bytes,
        0..v9_bytes.len(),
        ArchiveVersion::V9,
        UnitBinding::Millimeters(StandardUnit::Centimeters.into()),
        321,
    )
    .expect("archive-90 hatch pattern");
    assert_eq!(
        v9.distance_settings
            .map(|settings| settings.pattern_unit_system),
        Some(2)
    );
    assert_eq!(
        v9.distance_settings
            .map(|settings| settings.always_model_distances),
        Some(true)
    );
}
