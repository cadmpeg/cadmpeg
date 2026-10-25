// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{index_from_u32, u64_from_index};

use super::scope::{DesignFeatureKind, DesignParameterScope};

fn empty_scope(kind: DesignFeatureKind) -> serde_json::Value {
    serde_json::to_value(DesignParameterScope::empty("scope", kind, 1)).expect("serialize scope")
}

#[test]
fn flattened_scope_payloads_propagate_invalid_field_errors() {
    for (kind, field) in [
        (DesignFeatureKind::Extrude, "extrude_prologue"),
        (DesignFeatureKind::CoilPrimitive, "coil_extent"),
        (DesignFeatureKind::BaseFlange, "base_flange_operation"),
        (DesignFeatureKind::Loft, "path_feature_construction"),
    ] {
        let mut wire = empty_scope(kind);
        wire[field] = serde_json::json!(17);
        assert!(serde_json::from_value::<DesignParameterScope>(wire).is_err());
    }
}

#[test]
fn flattened_scope_frames_reject_partial_value_offset_pairs() {
    let transform = serde_json::json!([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0]
    ]);
    for (kind, prefix) in [
        (DesignFeatureKind::WorkPlane, "work_plane"),
        (DesignFeatureKind::JointOrigin, "joint_origin"),
    ] {
        let mut wire = empty_scope(kind.clone());
        wire[format!("{prefix}_reference")] = serde_json::json!(2);
        let error = serde_json::from_value::<DesignParameterScope>(wire)
            .expect_err("partial frame must fail");
        assert!(error.to_string().contains(prefix));
        for with_reference in [false, true] {
            let mut wire = empty_scope(kind.clone());
            wire[format!("{prefix}_transform")] = transform.clone();
            if with_reference {
                wire[format!("{prefix}_reference")] = serde_json::json!(2);
            }
            let decoded: DesignParameterScope =
                serde_json::from_value(wire.clone()).expect("complete frame");
            assert_eq!(
                serde_json::to_value(&decoded).expect("serialize frame"),
                wire
            );
            let mut pointers = crate::records::source_offsets::collect(
                &mut DesignParameterScope::empty("scope", kind.clone(), 1),
            );
            pointers.insert(format!("{prefix}_transform_offset"), 10);
            let mut names = vec![format!("{prefix}_transform_offset")];
            if with_reference {
                pointers.insert(format!("{prefix}_reference_offset"), 20);
                names.push(format!("{prefix}_reference_offset"));
            }
            let mut located = decoded;
            assert!(crate::records::source_offsets::restore(&mut located, &pointers).is_empty());
            super::test_support::assert_required_pointers(
                &located,
                &names.iter().map(String::as_str).collect::<Vec<_>>(),
            );
        }
    }
}

#[test]
fn flattened_sketch_entity_requires_all_identity_fields() {
    for (field, value) in [
        ("entity_id", serde_json::json!("entity:2")),
        ("entity_suffix", serde_json::json!(2)),
    ] {
        let mut wire = empty_scope(DesignFeatureKind::Sketch);
        wire[field] = value;
        let error = serde_json::from_value::<DesignParameterScope>(wire)
            .expect_err("partial identity must fail");
        assert!(error.to_string().contains("entity_id"));
    }
}

#[test]
fn absent_flattened_scope_payloads_preserve_the_wire() {
    for kind in [
        DesignFeatureKind::Extrude,
        DesignFeatureKind::CoilPrimitive,
        DesignFeatureKind::BaseFlange,
        DesignFeatureKind::Loft,
        DesignFeatureKind::Sweep,
        DesignFeatureKind::WorkPlane,
        DesignFeatureKind::JointOrigin,
        DesignFeatureKind::Sketch,
    ] {
        let scope = DesignParameterScope::empty("scope", kind, 1);
        let wire = serde_json::to_string(&scope).expect("serialize scope");
        let decoded: DesignParameterScope = serde_json::from_str(&wire).expect("empty payload");
        assert_eq!(
            serde_json::to_string(&decoded).expect("serialize scope"),
            wire
        );
    }
}

#[test]
fn revolve_opposite_angle_preserves_wire_and_rejects_partial_source_location() {
    let base = r#"{"operation":"join","angle":1.5,"angle_record_index":3"#;
    for tail in ["}", ",\"opposite_angle_record_index\":4}"] {
        let wire = format!("{base}{tail}");
        let value: crate::records::feature::path_features::DesignRevolveConstruction =
            serde_json::from_str(&wire).expect("revolve construction");
        assert_eq!(serde_json::to_string(&value).expect("revolve wire"), wire);
    }
    let mut located: crate::records::feature::path_features::DesignRevolveConstruction =
        serde_json::from_str(&format!("{base},\"opposite_angle_record_index\":4}}"))
            .expect("revolve construction");
    located.operation_offset = Some(12);
    located.angle_offset = Some(40);
    if let Some(opposite) = &mut located.opposite_angle {
        opposite.offset = Some(80);
    }
    super::test_support::assert_located_pointers(
        &located,
        &["operation_offset", "angle_offset", "opposite_angle_offset"],
    );
}

#[test]
fn extrude_prefixes_preserve_wire_and_reject_partial_locations() {
    let reference = r#"{"record_index":2,"trailing_zero_count":7"#;
    for fields in ["", ",\"operation_prefix_marker\":1"] {
        let wire = format!("{reference}{fields}}}");
        let value: crate::records::feature::extrude::DesignExtrudePrologueReference =
            serde_json::from_str(&wire).expect("prologue reference");
        assert_eq!(
            serde_json::to_string(&value).expect("prologue reference wire"),
            wire
        );
    }
    for marker in [0, 2, u8::MAX] {
        let wire = format!("{reference},\"operation_prefix_marker\":{marker}}}");
        let error = serde_json::from_str::<
            crate::records::feature::extrude::DesignExtrudePrologueReference,
        >(&wire)
        .expect_err("invalid prefix marker");
        assert!(error.to_string().contains("operation_prefix_marker"));
    }
    let mut located: crate::records::feature::extrude::DesignExtrudePrologueReference =
        serde_json::from_str(&format!("{reference},\"operation_prefix_marker\":1}}"))
            .expect("prologue reference");
    located.record_index_offset = Some(26);
    if let Some(marker) = &mut located.operation_prefix_marker {
        marker.offset = Some(37);
    }
    super::test_support::assert_located_pointers(
        &located,
        &["record_index_offset", "operation_prefix_marker_offset"],
    );
    for (layout, field, suffix, names) in [
        (
            "legacy_distance",
            "prefix_value",
            r#","operation":"join","extent_kind":2,"direction_reversed":false,"geometry_kind":1}"#,
            &[
                "operation_offset",
                "extent_kind_offset",
                "direction_reversed_offset",
                "geometry_kind_offset",
            ][..],
        ),
        (
            "legacy_shifted",
            "operation_prefix_marker",
            r#","operation":"join","direction_face_extend_values":[1,0],"side_extent_discriminators":[1,0],"direction_reversed":false,"solid_operation":true,"start":"profile_plane"}"#,
            &[
                "operation_offset",
                "side_extent_discriminator_offsets.0",
                "side_extent_discriminator_offsets.1",
                "direction_face_extend_offsets.0",
                "direction_face_extend_offsets.1",
                "direction_reversed_offset",
                "solid_operation_offset",
                "start_offset",
            ][..],
        ),
    ] {
        for value in [None, Some(i32::from(layout != "legacy_distance"))] {
            let fields = match value {
                Some(value) => format!(",\"{field}\":{value}"),
                None if layout == "legacy_distance" => format!(",\"{field}\":null"),
                None => String::new(),
            };
            let wire = format!("{{\"layout\":\"{layout}\"{fields}{suffix}");
            let value: crate::records::feature::extrude::DesignExtrudePrologue =
                serde_json::from_str(&wire).expect("extrude prologue");
            assert_eq!(
                serde_json::to_string(&value).expect("extrude prologue wire"),
                wire
            );
            let mut located = value;
            let mut next = 20;
            let mut pointers = std::collections::BTreeMap::new();
            let mut expected = names.to_vec();
            if value_field(&located).is_some() {
                expected.push(prefix_pointer(field));
            }
            for name in &expected {
                next += 4;
                pointers.insert((*name).to_owned(), next);
            }
            assert!(crate::records::source_offsets::restore(&mut located, &pointers).is_empty());
            super::test_support::assert_located_pointers(&located, &expected);
        }
        for invalid in [2, u8::MAX] {
            let wire = format!("{{\"layout\":\"{layout}\",\"{field}\":{invalid}{suffix}");
            let error = serde_json::from_str::<
                crate::records::feature::extrude::DesignExtrudePrologue,
            >(&wire)
            .expect_err("invalid prologue prefix");
            assert!(error.to_string().contains(field));
        }
    }
}

/// The prefix value or marker of a legacy prologue, when it stores one.
fn value_field(prologue: &crate::records::feature::extrude::DesignExtrudePrologue) -> Option<()> {
    match prologue {
        crate::records::feature::extrude::DesignExtrudePrologue::LegacyDistance {
            prefix_zero,
            ..
        } => prefix_zero.map(|_| ()),
        crate::records::feature::extrude::DesignExtrudePrologue::LegacyShifted {
            operation_prefix_marker,
            ..
        } => operation_prefix_marker.map(|_| ()),
        _ => None,
    }
}

/// The pointer of the prefix value or marker named `field`.
fn prefix_pointer(field: &str) -> &'static str {
    if field == "prefix_value" {
        "prefix_value_offset"
    } else {
        "operation_prefix_marker_offset"
    }
}

#[test]
fn mirror_references_preserve_wire_and_reject_partial_locations() {
    let prefix = r#"{"count":2,"count_record_index":11,"stitch_tolerance":0.001,"stitch_tolerance_record_index":12,"seed_group_record_index":20,"plane_group_record_index":30"#;
    let both = ",\"seed_feature_scope_record_index\":40,\"plane_scope_record_index\":50";
    for fields in [
        "",
        ",\"seed_feature_scope_record_index\":40",
        ",\"plane_scope_record_index\":50",
        both,
    ] {
        let wire = format!("{prefix}{fields}}}");
        let value: crate::records::feature::mirror::DesignMirrorConstruction =
            serde_json::from_str(&wire).expect("mirror construction");
        assert_eq!(serde_json::to_string(&value).expect("mirror wire"), wire);
    }
    let mut located: crate::records::feature::mirror::DesignMirrorConstruction =
        serde_json::from_str(&format!("{prefix}{both}}}")).expect("mirror construction");
    located.count_offset = Some(0);
    located.stitch_tolerance_offset = Some(0);
    for (reference, offset) in [
        (&mut located.seed_feature_scope_record_index, 100),
        (&mut located.plane_scope_record_index, 200),
    ] {
        if let Some(reference) = reference {
            reference.offset = Some(offset);
        }
    }
    super::test_support::assert_located_pointers(
        &located,
        &[
            "count_offset",
            "stitch_tolerance_offset",
            "seed_feature_reference_offset",
            "plane_reference_offset",
        ],
    );
}

#[test]
// Fixture fields are appended from the bounded table of explicit test cases.
#[allow(clippy::format_push_string)]
fn hole_construction_preserves_tangent_and_input_reference_wire() {
    let prefix = r#"{"point_record_index":55,"position":[1.25,-2.5,3.75],"direction":[0.0,0.0,1.0],"point_parameters":[0.125,-0.25],"reference_type":19"#;
    let fields = [
        ("tangent_point_data", "[-1.0,-1.0,-1.0]"),
        ("tangent_point_data_prefix", "127"),
    ];
    for mask in 0..4 {
        let mut wire = prefix.to_owned();
        for (index, (field, value)) in fields.iter().enumerate() {
            if mask & (1 << index) != 0 {
                wire.push_str(&format!(",\"{field}\":{value}"));
            }
        }
        wire.push_str(r#","input_record_indices":[378,379]}"#);
        let result =
            serde_json::from_str::<crate::records::feature::hole::DesignHoleConstruction>(&wire);
        if mask == 0 || mask == 3 {
            let mut located = result.expect("complete tangent form");
            assert_eq!(serde_json::to_string(&located).expect("hole wire"), wire);
            located.point_record_byte_offset = Some(10);
            located.position_offset = Some(35);
            located.direction_offset = Some(59);
            located.point_parameter_offsets = Some([83, 91]);
            located.reference_type_offset = Some(99);
            for (input, offset) in located.input_records.iter_mut().zip([129, 134]) {
                input.offset = Some(offset);
            }
            let mut names = vec![
                "point_record_byte_offset",
                "position_offset",
                "direction_offset",
                "point_parameter_offsets.0",
                "point_parameter_offsets.1",
                "reference_type_offset",
                "input_record_offsets.0",
                "input_record_offsets.1",
            ];
            if let Some(tangent) = &mut located.tangent_point_data {
                tangent.data.offset = Some(104);
                names.push("tangent_point_data_offset");
            }
            super::test_support::assert_located_pointers(&located, &names);
        } else {
            let error = result.expect_err("partial tangent form").to_string();
            for (field, _) in fields {
                assert!(error.contains(field));
            }
        }
    }
}

#[test]
// The resize uses only the explicit small lengths in this wire fixture.
#[allow(clippy::disallowed_methods)]
fn copied_body_rows_preserve_wire_and_reject_unequal_runs() {
    let wire = r#"{"body_group_record_index":501,"body_group_class_tag":"264","body_operand_record_indices":[502,504],"relation_record_index":503,"relation_class_tag":"264","source_body_entity_suffixes":[11,13],"copied_body_entity_suffixes":[12,14]}"#;
    let operation: crate::records::feature::body_ops::DesignCopyPasteBodiesOperation =
        serde_json::from_str(wire).expect("copy body rows");
    assert_eq!(
        serde_json::to_string(&operation).expect("copy body wire"),
        wire
    );
    assert_eq!(operation.bodies()[1].source.value, 13);
    assert_eq!(operation.bodies()[1].copied.value, 14);
    super::test_support::assert_located_at(
        operation,
        &[
            ("body_group_byte_offset", 100),
            ("relation_byte_offset", 200),
            ("body_operand_record_offsets.0", 126),
            ("body_operand_record_offsets.1", 137),
            ("source_body_entity_suffix_offsets.0", 225),
            ("source_body_entity_suffix_offsets.1", 255),
            ("copied_body_entity_suffix_offsets.0", 240),
            ("copied_body_entity_suffix_offsets.1", 270),
        ],
    );
    let value: serde_json::Value = serde_json::from_str(wire).expect("copy JSON");
    for field in [
        "body_operand_record_indices",
        "source_body_entity_suffixes",
        "copied_body_entity_suffixes",
    ] {
        for length in [0, 1, 3] {
            let mut invalid = value.clone();
            invalid[field]
                .as_array_mut()
                .expect("copy array")
                .resize(length, serde_json::json!(1));
            let error = serde_json::from_value::<
                crate::records::feature::body_ops::DesignCopyPasteBodiesOperation,
            >(invalid)
            .expect_err("unequal body runs")
            .to_string();
            assert!(error.contains(field), "{field}: {error}");
        }
    }
}

#[test]
fn scale_center_preserves_wire_and_rejects_partial_location() {
    for center in [
        None,
        Some(crate::records::identity::Sourced::at(
            crate::test_support::reals([1.25, -2.5, 3.75]),
            40,
        )),
    ] {
        let record = crate::records::feature::body_ops::DesignScaleOperation {
            body_group_record_index: 102,
            center_record_index: 105,
            center_position: center,
            uniform_factor: cadmpeg_ir::scalar::PositiveReal::new(2.5)
                .expect("checked fixture value"),
            uniform_factor_offset: Some(21),
        };
        let expected = match center {
            None => {
                r#"{"body_group_record_index":102,"center_record_index":105,"uniform_factor":2.5}"#
            }
            Some(_) => {
                r#"{"body_group_record_index":102,"center_record_index":105,"center_position":[1.25,-2.5,3.75],"uniform_factor":2.5}"#
            }
        };
        assert_eq!(serde_json::to_string(&record).unwrap(), expected);
        let read: crate::records::feature::body_ops::DesignScaleOperation =
            serde_json::from_str(expected).unwrap();
        let names: &[&str] = match center {
            None => &["uniform_factor_offset"],
            Some(_) => &["center_position_offset", "uniform_factor_offset"],
        };
        super::test_support::assert_located_pointers(&record, names);
        let pointers = crate::records::source_offsets::collect(&mut record.clone());
        let mut restored = read;
        assert!(crate::records::source_offsets::restore(&mut restored, &pointers).is_empty());
        assert_eq!(restored, record);
    }
}

#[test]
fn rectangular_pattern_rows_preserve_wire_and_reject_parallel_mismatch() {
    let transform = serde_json::json!([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0]
    ]);
    for count in [0, 1, 3] {
        let value = serde_json::json!({
            "record_indices": (0..count).collect::<Vec<u32>>(),
            "transforms": (0..count).map(|_| transform.clone()).collect::<Vec<_>>()
        });
        let wire: crate::records::feature::patterns::DesignRectangularPatternInstancesWire =
            serde_json::from_value(value.clone()).unwrap();
        let expected = serde_json::to_string(&wire).unwrap();
        let native: crate::records::feature::patterns::DesignRectangularPatternInstances =
            serde_json::from_str(&expected).unwrap();
        assert_eq!(native.instance_count(), index_from_u32(count));
        assert_eq!(serde_json::to_string(&native).unwrap(), expected);
        if count != 0 {
            let mut component = value.clone();
            component["component_occurrences"] = serde_json::json!({
                "component_guid": "00000001-1111-4111-8111-111111111111", "seed_occurrence_guid": "00000003-1111-4111-8111-111111111111",
                "generated_occurrence_guids": (1..count).map(|index| format!("{index:08}-2222-4222-8222-222222222222")).collect::<Vec<_>>()
            });
            let wire: crate::records::feature::patterns::DesignRectangularPatternInstancesWire =
                serde_json::from_value(component.clone()).unwrap();
            let expected = serde_json::to_string(&wire).unwrap();
            let native: crate::records::feature::patterns::DesignRectangularPatternInstances =
                serde_json::from_str(&expected).unwrap();
            assert_eq!(serde_json::to_string(&native).unwrap(), expected);
            component["component_occurrences"]["generated_occurrence_guids"] = serde_json::json!([
                "99999999-9999-4999-8999-999999999999",
                "99999999-9999-4999-8999-999999999999",
                "99999999-9999-4999-8999-999999999999"
            ]);
            assert!(serde_json::from_value::<
                crate::records::feature::patterns::DesignRectangularPatternInstances,
            >(component)
            .unwrap_err()
            .to_string()
            .contains("generated_occurrence_guids"));
        }
        for field in ["record_indices", "transforms"] {
            let mut invalid = value.clone();
            invalid[field]
                .as_array_mut()
                .unwrap()
                .push(if field == "transforms" {
                    transform.clone()
                } else {
                    serde_json::json!(0)
                });
            assert!(serde_json::from_value::<
                crate::records::feature::patterns::DesignRectangularPatternInstances,
            >(invalid)
            .unwrap_err()
            .to_string()
            .contains(field));
        }
    }
    let empty_component = serde_json::json!({
        "record_indices": [], "transforms": [],
        "component_occurrences": { "component_guid": "00000001-1111-4111-8111-111111111111", "seed_occurrence_guid": "00000003-1111-4111-8111-111111111111", "generated_occurrence_guids": [] }
    });
    assert!(serde_json::from_value::<
        crate::records::feature::patterns::DesignRectangularPatternInstances,
    >(empty_component)
    .unwrap_err()
    .to_string()
    .contains("seed"));
}

#[test]
fn edge_flange_rows_preserve_wire_and_reject_parallel_mismatch() {
    for count in [0, 1, 3] {
        let wire = crate::records::feature::sheet_metal::DesignEdgeFlangeOperationSerde {
            edge_wrapper_record_indices: (0..count).map(|index| 100 + index).collect(),
            edge_group_record_indices: (0..count).map(|index| 200 + index).collect(),
            edge_operand_record_indices: (0..count).map(|index| 203 + index).collect(),
            aggregate_group_record_index: 300,
            aggregate_operand_record_indices: (0..count).map(|index| 303 + index).collect(),
            height_owner_record_index: 400,
            height_extent: crate::records::feature::sheet_metal::DesignEdgeFlangeHeightExtent::Distance,
            angle_owner_record_index: 401,
            width_mode: Some(crate::records::feature::sheet_metal::DesignEdgeWidthMode::FullEdge),
            width_distance_owner_record_indices: Vec::new(),
            width_distance_owner_record_indices_by_edge: Vec::new(),
            auxiliary_reference_record_indices: Vec::new(),
            width_parameter_source:
                crate::records::feature::sheet_metal::DesignEdgeFlangeWidthParameterSource::EdgeWidth,
            settings_record_index: 402,
            bend_radius: 0.25,
            reference_side_code: 4,
            height_datum: crate::records::feature::sheet_metal::DesignSheetMetalHeightDatum::InnerFaces,
            bend_position: crate::records::feature::sheet_metal::DesignBendPosition::Adjacent,
        };
        let expected = serde_json::to_string(&wire).unwrap();
        let native: crate::records::feature::sheet_metal::DesignEdgeFlangeOperation =
            serde_json::from_str(&expected).unwrap();
        assert_eq!(
            native.selection.shape().edges().count(),
            index_from_u32(count)
        );
        assert_eq!(serde_json::to_string(&native).unwrap(), expected);
        for radius in [0.0, -1.0, f64::INFINITY, f64::NAN] {
            let mut invalid = wire.clone();
            invalid.bend_radius = radius;
            let error =
                crate::records::feature::sheet_metal::DesignEdgeFlangeOperation::try_from(invalid)
                    .expect_err("invalid bend radius");
            assert!(error.contains("bend_radius"));
        }
        for mode in [
            crate::records::feature::sheet_metal::DesignEdgeWidthMode::Symmetric,
            crate::records::feature::sheet_metal::DesignEdgeWidthMode::TwoSides,
            crate::records::feature::sheet_metal::DesignEdgeWidthMode::SymmetricPerEdge,
            crate::records::feature::sheet_metal::DesignEdgeWidthMode::TwoSidesPerEdge,
        ] {
            if count == 0
                && matches!(
                    mode,
                    crate::records::feature::sheet_metal::DesignEdgeWidthMode::SymmetricPerEdge
                        | crate::records::feature::sheet_metal::DesignEdgeWidthMode::TwoSidesPerEdge
                )
            {
                continue;
            }
            let mut width_wire = wire.clone();
            width_wire.width_mode = Some(mode);
            width_wire.width_distance_owner_record_indices = match mode {
                crate::records::feature::sheet_metal::DesignEdgeWidthMode::Symmetric => vec![600],
                crate::records::feature::sheet_metal::DesignEdgeWidthMode::TwoSides => {
                    vec![600, 601]
                }
                crate::records::feature::sheet_metal::DesignEdgeWidthMode::SymmetricPerEdge => {
                    (0..count).map(|index| 600 + index).collect()
                }
                crate::records::feature::sheet_metal::DesignEdgeWidthMode::TwoSidesPerEdge => {
                    (0..2 * count).map(|index| 600 + index).collect()
                }
                crate::records::feature::sheet_metal::DesignEdgeWidthMode::FullEdge => Vec::new(),
            };
            if mode == crate::records::feature::sheet_metal::DesignEdgeWidthMode::TwoSidesPerEdge {
                width_wire.width_distance_owner_record_indices_by_edge = (0..count)
                    .map(|index| [600 + 2 * index, 601 + 2 * index])
                    .collect();
            }
            for source in [
                crate::records::feature::sheet_metal::DesignEdgeFlangeWidthParameterSource::EdgeWidth,
                crate::records::feature::sheet_metal::DesignEdgeFlangeWidthParameterSource::EdgeOffset,
            ] {
                width_wire.width_parameter_source = source;
                let expected = serde_json::to_string(&width_wire).unwrap();
                let decoded = serde_json::from_str::<
                    crate::records::feature::sheet_metal::DesignEdgeFlangeOperation,
                >(&expected);
                if source
                    == crate::records::feature::sheet_metal::DesignEdgeFlangeWidthParameterSource::EdgeOffset
                    && mode != crate::records::feature::sheet_metal::DesignEdgeWidthMode::TwoSidesPerEdge
                {
                    assert!(decoded
                        .unwrap_err()
                        .to_string()
                        .contains("width_parameter_source"));
                } else {
                    assert_eq!(serde_json::to_string(&decoded.unwrap()).unwrap(), expected);
                }
            }
            width_wire.width_parameter_source =
                crate::records::feature::sheet_metal::DesignEdgeFlangeWidthParameterSource::EdgeWidth;
            if matches!(
                mode,
                crate::records::feature::sheet_metal::DesignEdgeWidthMode::SymmetricPerEdge
                    | crate::records::feature::sheet_metal::DesignEdgeWidthMode::TwoSidesPerEdge
            ) {
                let mut invalid = width_wire.clone();
                invalid.width_distance_owner_record_indices.push(999);
                if mode
                    == crate::records::feature::sheet_metal::DesignEdgeWidthMode::TwoSidesPerEdge
                {
                    invalid.width_distance_owner_record_indices.push(1000);
                    invalid
                        .width_distance_owner_record_indices_by_edge
                        .push([999, 1000]);
                }
                assert!(
                    crate::records::feature::sheet_metal::DesignEdgeFlangeOperation::try_from(
                        invalid
                    )
                    .unwrap_err()
                    .contains("selected edges")
                );
            }
            width_wire.height_extent =
                crate::records::feature::sheet_metal::DesignEdgeFlangeHeightExtent::ToObject {
                    target_group_record_index: 700,
                    target_operand_record_index: 703,
                    offset_owner_record_index: 710,
                    reference_record_indices: [720, 721],
                };
            assert!(
                crate::records::feature::sheet_metal::DesignEdgeFlangeOperation::try_from(
                    width_wire
                )
                .unwrap_err()
                .contains("height_extent")
            );
        }
        let mut to_object = wire.clone();
        to_object.height_extent =
            crate::records::feature::sheet_metal::DesignEdgeFlangeHeightExtent::ToObject {
                target_group_record_index: 700,
                target_operand_record_index: 703,
                offset_owner_record_index: 710,
                reference_record_indices: [720, 721],
            };
        let expected = serde_json::to_string(&to_object).unwrap();
        let native: crate::records::feature::sheet_metal::DesignEdgeFlangeOperation =
            serde_json::from_str(&expected).unwrap();
        assert_eq!(serde_json::to_string(&native).unwrap(), expected);
        for field in [
            "edge_wrapper_record_indices",
            "edge_group_record_indices",
            "edge_operand_record_indices",
            "aggregate_operand_record_indices",
        ] {
            let mut invalid = serde_json::to_value(&wire).unwrap();
            invalid[field]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!(999));
            assert!(serde_json::from_value::<
                crate::records::feature::sheet_metal::DesignEdgeFlangeOperation,
            >(invalid)
            .unwrap_err()
            .to_string()
            .contains(field));
        }
    }
}

#[test]
fn scope_reference_runs_preserve_wire_and_reject_partial_locations() {
    for values in [vec![10_u32], vec![10, 20, 30]] {
        let count = u64_from_index(values.len());
        let kind_offset = 21 + 11 * count;
        let scope = DesignParameterScope::try_new(super::scope::DesignParameterScopeDraft {
            id: "scope".to_owned(),
            byte_offset: 0,
            class_tag: "256".to_owned().try_into().unwrap(),
            record_index: 1,
            frame_length: kind_offset + 96,
            kind_offset,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: kind_offset + 16,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 9,
            reference_members: crate::records::identity::ReferenceRun::located(
                values
                    .iter()
                    .enumerate()
                    .map(|(ordinal, value)| crate::records::identity::Located {
                        value: *value,
                        offset: 14 + 11 * u64_from_index(ordinal),
                    })
                    .collect(),
            ),
            payload: DesignFeatureKind::Sketch.try_into().unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: "257".to_owned().try_into().unwrap(),
            paired_byte_offset: kind_offset + 96,
        })
        .expect("located scope");
        let wire = serde_json::to_string(&scope).unwrap();
        assert!(wire.contains(&format!(
            "\"reference_members\":{}",
            serde_json::to_string(&values).unwrap()
        )));
        assert!(!wire.contains("reference_member_offsets"));
        let read: DesignParameterScope = serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&read).unwrap(), wire);
        for scope in [&scope, &read] {
            assert_eq!(
                scope.reference_members().values().len(),
                scope.reference_members().len()
            );
            assert_eq!(
                scope
                    .reference_members()
                    .values_in(0..scope.reference_members().len())
                    .unwrap()
                    .copied()
                    .collect::<Vec<_>>(),
                values
            );
            assert!(scope
                .reference_members()
                .values_in(0..scope.reference_members().len() + 1)
                .is_none());
        }
        let mut names = vec![
            "byte_offset".to_owned(),
            "kind_offset".to_owned(),
            "history_state_id_offset".to_owned(),
            "feature_ordinal_offset".to_owned(),
            "reference_count_offset".to_owned(),
            "paired_byte_offset".to_owned(),
        ];
        names.extend((0..values.len()).map(|index| format!("reference_member_offsets.{index}")));
        super::test_support::assert_located_pointers(
            &scope,
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
        );
    }
}

#[test]
fn fixed_fillet_law_wire_preserves_scalar_order_and_rejects_partial_lanes() {
    for radius_count in [1_u32, 2, 3, 5] {
        let intermediate_count = radius_count.saturating_sub(2);
        let mut wire = serde_json::json!({
            "radii": (0..radius_count).map(|index| f64::from(index + 1)).collect::<Vec<_>>(),
            "radius_record_indexes": (0..radius_count).map(|index| 10 + index).collect::<Vec<_>>()
        });
        if intermediate_count != 0 {
            wire["intermediate_parameters"] = serde_json::json!((0..intermediate_count)
                .map(|index| f64::from(index + 1) / 4.0)
                .collect::<Vec<_>>());
            wire["intermediate_parameter_record_indexes"] = serde_json::json!((0
                ..intermediate_count)
                .map(|index| 20 + index)
                .collect::<Vec<_>>());
        }
        for with_tangency in [false, true] {
            let mut wire = wire.clone();
            if with_tangency {
                wire["tangency_weight"] = serde_json::json!({ "value": 1.0, "record_index": 5 });
            }
            let group: crate::records::feature::fixed_parameters::DesignFixedFilletGroup =
                serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(group.law().radii().count(), index_from_u32(radius_count));
            assert_eq!(
                group.law().intermediate().len(),
                index_from_u32(intermediate_count)
            );
            assert_eq!(serde_json::to_value(&group).unwrap(), wire);
            let mut pointers = (0..radius_count)
                .map(|index| {
                    (
                        format!("groups.0.radius_offsets.{index}"),
                        u64::from(100 + index * 8),
                    )
                })
                .chain((0..intermediate_count).map(|index| {
                    (
                        format!("groups.0.intermediate_parameter_offsets.{index}"),
                        u64::from(200 + index * 8),
                    )
                }))
                .collect::<Vec<_>>();
            if with_tangency {
                pointers.push(("groups.0.tangency_weight.value_offset".to_owned(), 50));
            }
            let pointers = pointers
                .iter()
                .map(|(name, offset)| (name.as_str(), *offset))
                .collect::<Vec<_>>();
            super::test_support::assert_located_at(
                crate::records::feature::fixed_parameters::DesignFixedFilletParameters {
                    groups: vec![group],
                },
                &pointers,
            );
            for field in [
                "radii",
                "radius_record_indexes",
                "intermediate_parameters",
                "intermediate_parameter_record_indexes",
            ] {
                let mut invalid = wire.clone();
                let mut column = invalid
                    .get(field)
                    .and_then(serde_json::Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                column.push(serde_json::json!(1));
                invalid[field] = serde_json::Value::Array(column);
                assert!(serde_json::from_value::<
                    crate::records::feature::fixed_parameters::DesignFixedFilletGroup,
                >(invalid)
                .unwrap_err()
                .to_string()
                .contains(field));
            }
            let mut invalid = wire;
            for field in ["radii", "radius_record_indexes"] {
                invalid[field] = serde_json::json!([]);
            }
            assert!(serde_json::from_value::<
                crate::records::feature::fixed_parameters::DesignFixedFilletGroup,
            >(invalid)
            .unwrap_err()
            .to_string()
            .contains("radii"));
        }
    }
    for radii in [vec![1.0], vec![1.0, 2.0, 3.0]] {
        let wire = serde_json::json!({
            "radius_record_indexes": vec![10; radii.len()], "radii": radii,
            "intermediate_parameters": [0.25, 0.5], "intermediate_parameter_record_indexes": [20, 21]
        });
        assert!(serde_json::from_value::<
            crate::records::feature::fixed_parameters::DesignFixedFilletGroup,
        >(wire)
        .unwrap_err()
        .to_string()
        .contains("intermediate_parameters"));
    }
}

#[test]
fn circular_pattern_axis_wire_preserves_shared_identity_and_rejects_partial_rows() {
    let inline = serde_json::json!({
        "kind": "inline", "origin": [1.0, 2.0, 3.0],
        "direction": [0.0, 0.0, 1.0]
    });
    let axis: crate::records::feature::patterns::DesignCircularPatternAxis =
        serde_json::from_value(inline.clone()).unwrap();
    assert_eq!(serde_json::to_value(&axis).unwrap(), inline);
    super::test_support::assert_located_at(
        circular_construction(axis),
        &[
            ("count_offset", 1),
            ("angle_offset", 2),
            ("axis.origin_offset", 12),
            ("axis.direction_offset", 36),
        ],
    );
    for count in [1_u32, 2] {
        for resolved in [false, true] {
            let mut wire = serde_json::json!({
                "kind": "historical_edge",
                "wrapper_record_indices": (0..count).map(|index| 10 + index).collect::<Vec<_>>(),
                "persistent_identities": [17]
            });
            if resolved {
                wire["resolved_origin"] =
                    serde_json::to_value(cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)).unwrap();
                wire["resolved_direction"] =
                    serde_json::to_value(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)).unwrap();
            }
            let axis: crate::records::feature::patterns::DesignCircularPatternAxis =
                serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(&axis).unwrap(), wire);
            let mut pointers = vec![
                ("count_offset".to_owned(), 1),
                ("angle_offset".to_owned(), 2),
            ];
            pointers.extend((0..count).map(|index| {
                (
                    format!("axis.identity_offsets.{index}"),
                    u64::from(100 + index * 8),
                )
            }));
            let pointers = pointers
                .iter()
                .map(|(name, offset)| (name.as_str(), *offset))
                .collect::<Vec<_>>();
            super::test_support::assert_located_at(circular_construction(axis), &pointers);
            for field in ["persistent_identities"] {
                let mut invalid = wire.clone();
                invalid[field]
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::json!(99));
                assert!(serde_json::from_value::<
                    crate::records::feature::patterns::DesignCircularPatternAxis,
                >(invalid)
                .unwrap_err()
                .to_string()
                .contains(field));
            }
            let mut invalid = wire.clone();
            invalid["persistent_identities"] = serde_json::json!([]);
            assert!(serde_json::from_value::<
                crate::records::feature::patterns::DesignCircularPatternAxis,
            >(invalid)
            .unwrap_err()
            .to_string()
            .contains("persistent_identities"));
            for field in ["resolved_origin", "resolved_direction"] {
                let mut invalid = wire.clone();
                invalid.as_object_mut().unwrap().remove("resolved_origin");
                invalid
                    .as_object_mut()
                    .unwrap()
                    .remove("resolved_direction");
                invalid[field] =
                    serde_json::to_value(cadmpeg_ir::math::Point3::new(0.0, 0.0, 1.0)).unwrap();
                let error = serde_json::from_value::<
                    crate::records::feature::patterns::DesignCircularPatternAxis,
                >(invalid)
                .unwrap_err()
                .to_string();
                assert!(error.contains("resolved_origin"));
                assert!(error.contains("resolved_direction"));
            }
        }
    }
}

#[test]
fn circular_pattern_inline_axis_native_wire_refuses_zero_direction() {
    let wire = serde_json::json!({
        "kind": "inline", "origin": [1.0, 2.0, 3.0], "origin_offset": 12,
        "direction": [0.0, 0.0, 0.0], "direction_offset": 36
    });
    assert!(
        serde_json::from_value::<crate::records::feature::patterns::DesignCircularPatternAxis>(
            wire
        )
        .is_err()
    );
}

#[test]
fn circular_pattern_inline_axis_native_wire_normalizes_displacement() {
    let wire = serde_json::json!({
        "kind": "inline", "origin": [1.0, 2.0, 3.0], "origin_offset": 12,
        "direction": [0.0, 0.0, 2.0], "direction_offset": 36
    });
    let axis: crate::records::feature::patterns::DesignCircularPatternAxis =
        serde_json::from_value(wire).expect("nonzero displacement");
    let crate::records::feature::patterns::DesignCircularPatternAxis::Inline { direction, .. } =
        axis
    else {
        panic!("inline axis");
    };
    assert_eq!(
        *direction.as_raw(),
        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
    );
}

#[test]
fn circular_pattern_resolved_axis_native_wire_refuses_zero_direction() {
    let wire = serde_json::json!({
        "kind": "historical_edge",
        "wrapper_record_indices": [10],
        "persistent_identities": [17],
        "identity_offsets": [100],
        "resolved_origin": {"x": 1.0, "y": 2.0, "z": 3.0},
        "resolved_direction": {"x": 0.0, "y": 0.0, "z": 0.0}
    });
    assert!(
        serde_json::from_value::<crate::records::feature::patterns::DesignCircularPatternAxis>(
            wire
        )
        .is_err()
    );
}

#[test]
fn mirror_plane_wire_rejects_partial_placement() {
    let prefix = r#"{"count":2,"count_record_index":11,"stitch_tolerance":0.001,"stitch_tolerance_record_index":12,"seed_group_record_index":20,"plane_group_record_index":30"#;
    let origin = serde_json::to_string(&cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)).unwrap();
    let normal = serde_json::to_string(&cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)).unwrap();
    for fields in [
        String::new(),
        format!(",\"plane_origin\":{origin},\"plane_normal\":{normal}"),
    ] {
        let wire = format!("{prefix}{fields}}}");
        let construction: crate::records::feature::mirror::DesignMirrorConstruction =
            serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&construction).unwrap(), wire);
    }
    for (field, value) in [("plane_origin", origin), ("plane_normal", normal)] {
        let invalid = format!("{prefix},\"{field}\":{value}}}");
        let error =
            serde_json::from_str::<crate::records::feature::mirror::DesignMirrorConstruction>(
                &invalid,
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains("plane_origin"));
        assert!(error.contains("plane_normal"));
    }
}

#[test]
fn coil_selection_preserves_wire_and_rejects_dependent_fields_without_identity() {
    let persistent = r#"{"kind":"persistent","asset_id":"00000001-1111-4111-8111-111111111111","context_id":"00000002-1111-4111-8111-111111111111","identity_record_index":3,"primary_identity":7"#;
    for fields in [
        "",
        ",\"secondary_identity\":11",
        ",\"secondary_identity\":11,\"curve_secondary_identity\":0",
        ",\"secondary_identity\":11,\"curve_secondary_identity\":13",
    ] {
        let wire = format!("{persistent}{fields}}}");
        let selection: crate::records::feature::coil::DesignCoilSelection =
            serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&selection).unwrap(), wire);
    }
    let wire = format!("{persistent},\"curve_secondary_identity\":13}}");
    let error = serde_json::from_str::<crate::records::feature::coil::DesignCoilSelection>(&wire)
        .unwrap_err()
        .to_string();
    assert!(error.contains("secondary_identity"));
    assert!(error.contains("curve_secondary_identity"));

    let face = r#"{"kind":"face_recipe","asset_id":"00000001-1111-4111-8111-111111111111","context_id":"00000002-1111-4111-8111-111111111111","recipe_record_index":3,"recipe_id":"recipe","recipe_kind":"#;
    for kind in ["face", "bounded_face"] {
        for fields in [
            "",
            ",\"design_id\":\"body\"",
            ",\"design_id\":\"body\",\"design_selector\":{\"value\":2}",
        ] {
            let wire = format!("{face}\"{kind}\"{fields}}}");
            let mut selection: crate::records::feature::coil::DesignCoilSelection =
                serde_json::from_str(&wire).unwrap();
            if let crate::records::feature::coil::DesignCoilSelection::FaceRecipe {
                design: Some(design),
                ..
            } = &mut selection
            {
                if let Some(selector) = &mut design.selector {
                    selector.byte_offset = Some(60);
                    assert_eq!(selector.value, 2);
                    assert_eq!(selector.byte_offset, Some(60));
                }
            }
            assert_eq!(serde_json::to_string(&selection).unwrap(), wire);
        }
    }
    let wire = format!("{face}\"face\",\"design_selector\":{{\"value\":2}}}}");
    let error = serde_json::from_str::<crate::records::feature::coil::DesignCoilSelection>(&wire)
        .unwrap_err()
        .to_string();
    assert!(error.contains("design_id"));
    assert!(error.contains("design_selector"));
    for kind in ["body", "edge", "vertex"] {
        let wire = format!("{face}\"{kind}\"}}");
        let error =
            serde_json::from_str::<crate::records::feature::coil::DesignCoilSelection>(&wire)
                .unwrap_err()
                .to_string();
        assert!(error.contains("recipe_kind"));
    }
}

#[test]
fn legacy_extrude_constants_and_geometry_preserve_wire() {
    for geometry_kind in [0, 1] {
        let wire = format!(
            r#"{{"layout":"legacy_distance","prefix_value":0,"operation":"join","extent_kind":2,"direction_reversed":false,"geometry_kind":{geometry_kind}}}"#
        );
        let prologue: crate::records::feature::extrude::DesignExtrudePrologue =
            serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&prologue).unwrap(), wire);
        assert_eq!(
            prologue.extent(),
            Some(crate::records::feature::extrude::DesignExtrudeExtent::OneSidedDistance)
        );
        assert_eq!(prologue.solid_operation(), geometry_kind == 1);
        super::test_support::assert_located_at(
            prologue,
            &[
                ("prefix_value_offset", 21),
                ("operation_offset", 25),
                ("extent_kind_offset", 29),
                ("direction_reversed_offset", 33),
                ("geometry_kind_offset", 34),
            ],
        );
        for (field, before, after) in [
            (
                "prefix_value",
                "\"prefix_value\":0".to_owned(),
                "\"prefix_value\":1".to_owned(),
            ),
            (
                "extent_kind",
                "\"extent_kind\":2".to_owned(),
                "\"extent_kind\":1".to_owned(),
            ),
            (
                "geometry_kind",
                format!("\"geometry_kind\":{geometry_kind}"),
                "\"geometry_kind\":2".to_owned(),
            ),
        ] {
            let invalid = wire.replace(&before, &after);
            let error = serde_json::from_str::<
                crate::records::feature::extrude::DesignExtrudePrologue,
            >(&invalid)
            .unwrap_err()
            .to_string();
            assert!(error.contains(field));
        }
    }
}

#[test]
fn coil_placement_derives_only_the_encoded_identity_matrix() {
    let prefix = r#"{"selection_record_index":1,"selection_class_tag":"353","selection":{"kind":"persistent","asset_id":"00000001-1111-4111-8111-111111111111","context_id":"00000002-1111-4111-8111-111111111111","identity_record_index":2,"primary_identity":3},"transform_record_index":4,"transform_class_tag":"450""#;
    let identity = "[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]]";
    let translated = "[[1.0,0.0,0.0,2.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]]";
    for matrix in [None, Some(identity), Some(translated)] {
        let wire = match matrix {
            None => format!("{prefix}}}"),
            Some(matrix) => format!("{prefix},\"transform\":{matrix}}}"),
        };
        let placement: crate::records::feature::coil::DesignCoilPlacement =
            serde_json::from_str(&wire).expect("coil placement");
        assert_eq!(
            serde_json::to_string(&placement).expect("coil placement wire"),
            wire
        );
        assert_eq!(placement.explicit_transform.is_some(), matrix.is_some());
        if matrix.is_none() {
            assert_eq!(
                placement.transform(),
                &crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY
            );
        }
        let mut pointers = vec![
            ("coil_placement.selection_record_byte_offset", 0),
            ("coil_placement.transform_record_byte_offset", 5),
        ];
        if matrix.is_some() {
            pointers.push(("coil_placement.transform_offset", 55));
        }
        super::test_support::assert_located_at(
            crate::records::feature::coil::DesignCoilScope {
                placement: Some(placement),
                ..Default::default()
            },
            &pointers,
        );
    }
}

#[test]
fn move_form_preserves_its_closed_integer_wire_domain() {
    for code in [1, 5] {
        let wire = code.to_string();
        let form: crate::records::feature::direct_face::DesignMoveForm =
            serde_json::from_str(&wire).expect("move form");
        assert_eq!(serde_json::to_string(&form).expect("move form wire"), wire);
    }
    for code in [0, 2, 3, 4, 6, u32::MAX] {
        let error = serde_json::from_str::<crate::records::feature::direct_face::DesignMoveForm>(
            &code.to_string(),
        )
        .expect_err("invalid move form");
        assert!(error.to_string().contains("form"));
    }
}

#[test]
fn mirror_scope_tolerance_pairs_repeated_markers_with_their_locations() {
    for (marker, repeated) in [(61, None), (89, Some(59)), (94, Some(58)), (100, Some(59))] {
        let wire =
            format!("{{\"marker\":{marker},\"first_reference\":12,\"second_reference\":11}}");
        let lane: crate::records::feature::mirror::DesignMirrorScopeTolerance =
            serde_json::from_str(&wire).expect("mirror scalar lane");
        assert_eq!(
            serde_json::to_string(&lane).expect("mirror scalar lane wire"),
            wire
        );
        assert_eq!(lane.marker.code(), marker);
        let mut construction = mirror_with_scope_tolerance();
        construction.tolerance_source =
            crate::records::feature::mirror::DesignMirrorToleranceSource::Scope(lane);
        let mut pointers = vec![
            ("count_offset", 0),
            ("stitch_tolerance_offset", 51),
            ("stitch_tolerance_scope.marker_offset", 47),
            ("stitch_tolerance_scope.first_reference_offset", 63),
            ("stitch_tolerance_scope.second_reference_offset", 76),
        ];
        if let Some(repeated) = repeated {
            pointers.push(("stitch_tolerance_scope.repeated_marker_offset", repeated));
        }
        super::test_support::assert_located_at(construction, &pointers);
    }
    for marker in [0, 90] {
        let wire =
            format!("{{\"marker\":{marker},\"first_reference\":12,\"second_reference\":11}}");
        let error = serde_json::from_str::<
            crate::records::feature::mirror::DesignMirrorScopeTolerance,
        >(&wire)
        .expect_err("invalid mirror scalar lane");
        assert!(error.to_string().contains("marker"));
    }
}

const MIRROR_SCOPE_TOLERANCE_WIRE: &str = r#"{"count":2,"count_record_index":11,"stitch_tolerance":0.001,"stitch_tolerance_scope":{"marker":89,"first_reference":12,"second_reference":11},"seed_group_record_index":20,"plane_group_record_index":30}"#;

fn mirror_with_scope_tolerance() -> crate::records::feature::mirror::DesignMirrorConstruction {
    serde_json::from_str(MIRROR_SCOPE_TOLERANCE_WIRE).expect("inline mirror tolerance")
}

#[test]
fn mirror_derives_count_and_requires_one_tolerance_carrier() {
    let wire = MIRROR_SCOPE_TOLERANCE_WIRE;
    let construction: crate::records::feature::mirror::DesignMirrorConstruction =
        serde_json::from_str(wire).expect("inline mirror tolerance");
    assert_eq!(
        serde_json::to_string(&construction).expect("inline mirror tolerance wire"),
        wire
    );
    let source: serde_json::Value = serde_json::from_str(wire).expect("mirror wire");
    for count in [0, 1, 3, u32::MAX] {
        let mut invalid = source.clone();
        invalid["count"] = count.into();
        let error = serde_json::from_value::<
            crate::records::feature::mirror::DesignMirrorConstruction,
        >(invalid)
        .expect_err("fixed mirror count");
        assert!(error.to_string().contains("count"));
    }
    for present in [false, true] {
        let mut invalid = source.clone();
        if present {
            invalid["stitch_tolerance_record_index"] = 12.into();
        } else {
            invalid
                .as_object_mut()
                .expect("mirror object")
                .remove("stitch_tolerance_scope");
        }
        let error = serde_json::from_value::<
            crate::records::feature::mirror::DesignMirrorConstruction,
        >(invalid)
        .expect_err("one mirror tolerance carrier");
        assert!(error.to_string().contains("stitch_tolerance_record_index"));
        assert!(error.to_string().contains("stitch_tolerance_scope"));
    }
}

#[test]
fn combine_requires_boolean_operation_local_target_and_nonempty_tools() {
    for operation in ["join", "cut", "intersect"] {
        for tools in [
            r#"[{"record_index":2}]"#,
            r#"[{"record_index":2},{"record_index":3}]"#,
        ] {
            let wire = format!("{{\"form\":\"standard\",\"operation\":\"{operation}\",\"keep_tools\":false,\"target\":{{\"record_index\":1}},\"tools\":{tools}}}");
            let combine: crate::records::feature::combine::DesignCombineOperation =
                serde_json::from_str(&wire).expect("combine operation");
            assert_eq!(
                serde_json::to_string(&combine).expect("combine operation wire"),
                wire
            );
            super::test_support::assert_located_at(
                combine,
                &[("operation_offset", 20), ("keep_tools_offset", 25)],
            );
        }
    }
    let base = serde_json::json!({
        "form": "standard", "operation": "join",
        "keep_tools": false,
        "target": {"record_index": 1}, "tools": [{"record_index": 2}]
    });
    for (field, value) in [
        ("operation", serde_json::json!("new_body")),
        ("tools", serde_json::json!([])),
    ] {
        let mut invalid = base.clone();
        invalid[field] = value;
        let error = serde_json::from_value::<
            crate::records::feature::combine::DesignCombineOperation,
        >(invalid)
        .expect_err("invalid combine form");
        assert!(error.to_string().contains(field));
    }
    let mut external_target = base;
    external_target["target"]["external_identity"] = serde_json::json!({
        "selector_asset_id": "00000004-1111-4111-8111-111111111111",
        "selector_context_id": "00000005-1111-4111-8111-111111111111",
        "occurrence_reference": 1,
        "external_body_reference": 2,
        "external_segment": 1,
        "external_asset_id": "00000004-1111-4111-8111-111111111111",
        "external_link_name": "link",
        "tail_values": [0, 0]
    });
    let error = serde_json::from_value::<crate::records::feature::combine::DesignCombineOperation>(
        external_target,
    )
    .expect_err("local combine target");
    assert!(error.to_string().contains("target.external_identity"));
}

#[test]
fn thread_nominal_size_preserves_spelling_and_derives_numeric_wire_value() {
    let wire = |text: &str, number: &str| {
        format!("{{\"form\":\"standard\",\"designation\":\"M1\",\"nominal_size_text\":\"{text}\",\"nominal_size\":{number},\"profile\":\"ISO Metric profile\",\"major_diameter\":1.0,\"minor_diameter\":0.5,\"pitch\":0.1,\"pitch_diameter\":0.75,\"face_group_record_indices\":[10]}}")
    };
    for (text, number) in [
        ("1.0", "1.0"),
        ("+1.00", "1.0"),
        ("1.25e1", "12.5"),
        ("0.125", "0.125"),
    ] {
        let json = wire(text, number);
        let thread: crate::records::feature::thread::DesignThreadConstruction =
            serde_json::from_str(&json).expect("thread nominal size");
        assert_eq!(thread.nominal_size.text(), text);
        assert_eq!(
            serde_json::to_string(&thread).expect("thread nominal-size wire"),
            json
        );
        super::test_support::assert_located_at(thread, &[("designation_offset", 38)]);
    }
    for text in ["", "-", "0", "-0.0", "-1", "NaN", "inf", "1e9999"] {
        let error =
            serde_json::from_str::<crate::records::feature::thread::DesignThreadConstruction>(
                &wire(text, "1.0"),
            )
            .expect_err("invalid nominal-size spelling");
        assert!(error.to_string().contains("nominal_size_text"));
    }
    let error = serde_json::from_str::<crate::records::feature::thread::DesignThreadConstruction>(
        &wire("1.0", "2.0"),
    )
    .expect_err("derived nominal size");
    assert!(error.to_string().contains("nominal_size"));
    assert!(error.to_string().contains("nominal_size_text"));
    let compact = wire("1.0", "1.0").replace("\"standard\"", "\"compact\"");
    for index in [1, u32::MAX] {
        let json = compact.replace(
            "\"face_group_record_indices\"",
            &format!("\"trailing_reference_record_index\":{index},\"face_group_record_indices\""),
        );
        let thread: crate::records::feature::thread::DesignThreadConstruction =
            serde_json::from_str(&json).expect("compact trailer reference");
        assert_eq!(
            serde_json::to_string(&thread).expect("compact trailer wire"),
            json
        );
        super::test_support::assert_located_at(
            thread,
            &[
                ("designation_offset", 38),
                ("trailing_reference_offset", 100),
            ],
        );
    }
    let invalid = compact.replace(
        "\"face_group_record_indices\"",
        "\"trailing_reference_record_index\":0,\"face_group_record_indices\"",
    );
    let error =
        serde_json::from_str::<crate::records::feature::thread::DesignThreadConstruction>(&invalid)
            .expect_err("nonzero compact trailer reference");
    assert!(error
        .to_string()
        .contains("trailing_reference_record_index"));
}

#[test]
fn vertex_recipe_resolution_preserves_wire_and_rejects_partial_pairs() {
    use crate::records::feature::work_geometry::{
        DesignVertexRecipe, DesignVertexResolution, DesignWorkPlaneConstruction,
    };

    let base = serde_json::json!({
        "record_index": 2, "class_tag": "369",
        "paired_class_tag": "261",
        "recipe_record_index": 5,
        "recipe_id": "vertex",
        "recipe_prefix_bytes": "00ff", "recipe_references": [],
        "recipe_program": [0],
        "next_record_index": 7
    });
    for resolution in [None, Some((i64::MIN, 0)), Some((i64::MAX, i64::MAX))] {
        let mut wire = base.clone();
        if let Some((state, slot)) = resolution {
            wire["recipe_state_id"] = state.into();
            wire["resolved_vertex_slot"] = slot.into();
        }
        let decoded: DesignVertexRecipe =
            serde_json::from_value(wire.clone()).expect("valid recipe");
        assert_eq!(
            serde_json::to_value(&decoded).expect("serialize recipe"),
            wire
        );
        assert_eq!(
            decoded
                .resolution
                .map(|value| (value.state_id, value.vertex_slot())),
            resolution
        );
        super::test_support::assert_located_at(
            decoded,
            &[
                ("byte_offset", 10),
                ("paired_byte_offset", 20),
                ("recipe_record_byte_offset", 30),
                ("recipe_program_offset", 43),
                ("next_byte_offset", 50),
            ],
        );
        let mut plane_inputs: [serde_json::Value; 3] = std::array::from_fn(|_| wire.clone());
        if let Some((_, slot)) = resolution {
            for (ordinal, input) in plane_inputs.iter_mut().enumerate() {
                input["resolved_vertex_slot"] = if slot == 0 {
                    i64::try_from(ordinal).expect("fixture value fits i64")
                } else {
                    slot - i64::try_from(ordinal).expect("fixture value fits i64")
                }
                .into();
            }
        }
        let plane_wire = serde_json::json!({
            "kind": "three_point", "placement_record_index": 9,
            "inputs": plane_inputs
        });
        let plane: DesignWorkPlaneConstruction =
            serde_json::from_value(plane_wire.clone()).expect("three-point plane");
        assert_eq!(
            serde_json::to_value(&plane).expect("serialize plane"),
            plane_wire
        );
        let mut wrong_kind = plane_wire.clone();
        wrong_kind["kind"] = "two_point".into();
        assert!(
            serde_json::from_value::<DesignWorkPlaneConstruction>(wrong_kind).is_err(),
            "unknown work-plane construction kind"
        );
    }
    for (state, slot) in [(Some(4), None), (None, Some(0)), (Some(4), Some(-1))] {
        let mut wire = base.clone();
        if let Some(state) = state {
            wire["recipe_state_id"] = state.into();
        }
        if let Some(slot) = slot {
            wire["resolved_vertex_slot"] = slot.into();
        }
        let error =
            serde_json::from_value::<DesignVertexRecipe>(wire).expect_err("invalid resolution");
        assert!(error.to_string().contains("resolved_vertex_slot"));
    }
    assert!(DesignVertexResolution::new(4, -1).is_none());
}

#[test]
fn feature_kind_classifies_nonempty_native_names_at_construction() {
    for name in [
        "Fillet",
        "Esboço",
        "Extrusão",
        "Thread",
        "Unsupported",
        "fillet",
        " ",
    ] {
        let kind = DesignFeatureKind::try_from(name.to_owned()).expect("nonempty name");
        assert_eq!(kind.as_str(), name);
        assert_eq!(
            serde_json::to_value(&kind).expect("serialize kind"),
            serde_json::json!(name)
        );
        let decoded: DesignFeatureKind =
            serde_json::from_value(serde_json::json!(name)).expect("decode kind");
        assert_eq!(decoded, kind);
        assert_eq!(
            matches!(kind, DesignFeatureKind::Native(_)),
            matches!(name, "Unsupported" | "fillet" | " ")
        );
    }
    assert!(DesignFeatureKind::try_from(String::new()).is_err());
    let error = serde_json::from_value::<DesignFeatureKind>(serde_json::json!(""))
        .expect_err("empty name rejected");
    assert!(error.to_string().contains("kind"));
}

#[test]
fn scope_feature_ordinal_preserves_positive_wire_values_and_rejects_zero() {
    let base = empty_scope(DesignFeatureKind::Fillet);
    for ordinal in [1, u32::MAX] {
        let mut wire = base.clone();
        wire["feature_ordinal"] = ordinal.into();
        let scope: DesignParameterScope =
            serde_json::from_value(wire.clone()).expect("positive ordinal");
        assert_eq!(scope.feature_ordinal.get(), ordinal);
        assert_eq!(serde_json::to_value(scope).expect("serialize scope"), wire);
    }
    let mut wire = base;
    wire["feature_ordinal"] = 0.into();
    let error =
        serde_json::from_value::<DesignParameterScope>(wire).expect_err("zero ordinal rejected");
    assert!(error.to_string().contains("feature_ordinal"));
}

#[test]
fn scope_history_state_offset_is_derived_and_wire_mismatches_are_rejected() {
    for kind_offset in [32_u64, 100, u64::MAX - 88] {
        let mut scope = DesignParameterScope::empty("scope", DesignFeatureKind::Sketch, 1);
        scope
            .try_edit(|draft| {
                draft.byte_offset = kind_offset - 32;
                draft.frame_length = 120;
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.reference_count_offset = draft.byte_offset + 9;
                draft.layout_fixture_references();
                draft.kind_offset = kind_offset;
                draft.feature_ordinal_offset = kind_offset + 16;
            })
            .unwrap();
        let wire = serde_json::to_value(&scope).expect("serialize scope");
        assert!(wire.get("history_state_id_offset").is_none());
        let pointers = crate::records::source_offsets::collect(&mut scope.clone());
        assert_eq!(pointers["history_state_id_offset"], kind_offset - 8);
        assert!(scope.located_history_state_offset_holds());
        let mut decoded: DesignParameterScope =
            serde_json::from_value(wire.clone()).expect("valid derived offset");
        assert_eq!(
            serde_json::to_value(&decoded).expect("serialize scope"),
            wire
        );
        assert!(crate::records::source_offsets::restore(&mut decoded, &pointers).is_empty());
        decoded.restore_frame_length(120);
        assert_eq!(decoded, scope);
        let mut bad = pointers;
        bad.insert("history_state_id_offset".to_owned(), kind_offset - 7);
        let mut moved = scope.clone();
        crate::records::source_offsets::restore(&mut moved, &bad);
        assert!(!moved.located_history_state_offset_holds());
    }
}

#[test]
fn bend_radius_requires_a_positive_finite_value() {
    for value in [0.0, -0.0, -1.0, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        assert!(cadmpeg_ir::scalar::PositiveReal::new(value).is_none());
    }
    for radius in [f64::MIN_POSITIVE, 0.25, f64::MAX] {
        let value = cadmpeg_ir::scalar::PositiveReal::new(radius).expect("positive finite radius");
        assert_eq!(value.get(), radius);
        let wire = serde_json::json!({
            "edge_wrapper_record_index": 1, "edge_group_record_index": 2,
            "edge_operand_record_index": 5, "aggregate_group_record_index": 6,
            "aggregate_operand_record_index": 9,
            "parameter_owners": {"kind": "gap_length", "gap_owner_record_index": 10, "length_owner_record_index": 11},
            "settings_record_index": 12, "bend_radius": radius,
            "bend_position": "adjacent", "flipped": false
        });
        let operation: crate::records::feature::sheet_metal::DesignHemOperation =
            serde_json::from_value(wire.clone()).expect("valid radius");
        assert_eq!(operation.bend_radius.get(), radius);
        assert_eq!(
            serde_json::to_value(&operation).expect("serialize hem"),
            wire
        );
        super::test_support::assert_located_at(operation, &[("bend_radius_offset", 100)]);
        for invalid in [0.0, -1.0] {
            let mut bad = wire.clone();
            bad["bend_radius"] = invalid.into();
            let error = serde_json::from_value::<
                crate::records::feature::sheet_metal::DesignHemOperation,
            >(bad)
            .expect_err("invalid bend radius");
            assert!(error.to_string().contains("bend_radius"));
        }
    }
}

#[test]
fn rectangular_pattern_sidecar_rejects_invalid_axis_counts() {
    for (u_count, v_count, u_extent, v_extent) in [
        (0, 2, 0.0, 1.0),
        (2, 0, 1.0, 0.0),
        (1, 1, 0.0, 0.0),
        (1, 2, 1.0, 1.0),
        (2, 1, 0.0, 0.0),
        (2, 1, 1.0, 1.0),
        (1, 2, 0.0, 0.0),
    ] {
        let wire = serde_json::json!({
            "u_count": u_count, "v_count": v_count,
            "u_extent": u_extent, "v_extent": v_extent,
            "owner_record_indices": [1, 2, 3, 4],
            "value_offsets": [10, 20, 30, 40]
        });
        assert!(
            serde_json::from_value::<super::patterns::DesignRectangularPatternConstruction>(wire)
                .is_err()
        );
    }
}

#[test]
fn rectangular_pattern_sidecar_preserves_signed_spans() {
    for (u_count, v_count, u_extent, v_extent) in [(3, 1, -10.0, 0.0), (2, 2, 1.0, -1.0)] {
        let wire = serde_json::json!({
            "u_count": u_count, "v_count": v_count,
            "u_extent": u_extent, "v_extent": v_extent,
            "owner_record_indices": [1, 2, 3, 4]
        });
        let record: super::patterns::DesignRectangularPatternConstruction =
            serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&record).unwrap(), wire);
        super::test_support::assert_located_at(
            record,
            &[
                ("value_offsets.0", 10),
                ("value_offsets.1", 20),
                ("value_offsets.2", 30),
                ("value_offsets.3", 40),
            ],
        );
    }
}

#[test]
fn surface_trim_sidecar_requires_a_nonempty_cell_table() {
    let entry = serde_json::json!({"record_index": 4, "record_reference_offset": 0,
        "ordinal": 1, "ordinal_offset": 0});
    let mut wire = serde_json::json!({"id": "trim", "scope_record_index": 1,
        "selection_record_index": 2, "selection_byte_offset": 0,
        "selection_next_record_index": 3, "selection_next_byte_offset": 0,
        "chain_records": [
            {"record_index": 3, "byte_offset": 0, "class_tag": "288", "frame_length": 11},
            {"record_index": 6, "byte_offset": 11, "class_tag": "271", "frame_length": 11}
        ], "cell_table_record_index": 4, "cell_table_byte_offset": 0,
        "cell_table_class_tag": "325", "cell_table_frame_length": 0,
        "cell_table_paired_class_tag": "257", "cell_table_paired_byte_offset": 0,
        "cell_count_offset": 0, "cell_entries": [entry],
        "trailing_value": 1, "trailing_value_offset": 0, "trailing_zero_offset": 0});
    let record: super::surface_ops::DesignSurfaceTrimOperation =
        serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(record).unwrap(), wire);
    for count in [0, 1, 3] {
        let mut invalid_chain = wire.clone();
        invalid_chain["chain_records"] =
            serde_json::Value::Array(vec![wire["chain_records"][0].clone(); count]);
        assert!(
            serde_json::from_value::<super::surface_ops::DesignSurfaceTrimOperation>(invalid_chain)
                .is_err()
        );
    }
    wire["cell_entries"] = serde_json::json!([]);
    assert!(
        serde_json::from_value::<super::surface_ops::DesignSurfaceTrimOperation>(wire).is_err()
    );
}

mod scalars;

mod fillet_law;

mod combine;

mod copied_bodies;

mod admitted_scalars;

#[test]
fn required_surface_operations_reject_absence() {
    for (kind, field) in [
        (DesignFeatureKind::SurfaceStitch, "surface_stitch_operation"),
        (DesignFeatureKind::SurfaceRuled, "ruled_surface_operation"),
    ] {
        assert!(super::scope::DesignScopePayload::try_from(kind.clone()).is_err());
        let mut wire = empty_scope(DesignFeatureKind::Sketch);
        wire["kind"] = serde_json::json!(kind.as_str());
        let error = serde_json::from_value::<DesignParameterScope>(wire).unwrap_err();
        assert!(error.to_string().contains(field));
    }
}

#[test]
fn parameter_scope_layout_rejects_invalid_admission_and_preserves_failed_edits() {
    use super::scope::DesignParameterScopeDraft;
    let scope = DesignParameterScope::empty("scope", DesignFeatureKind::Sketch, 1);
    let invalid: &[fn(&mut DesignParameterScopeDraft)] = &[
        |draft| draft.byte_offset = u64::MAX,
        |draft| draft.frame_length = 89,
        |draft| draft.paired_byte_offset += 1,
        |draft| draft.kind_offset = draft.byte_offset,
        |draft| draft.feature_ordinal_offset = draft.kind_offset,
        |draft| draft.feature_ordinal_offset += 1,
        |draft| draft.previous_history_state_id = Some(1),
        |draft| draft.previous_history_state_id_offset = Some(1),
        |draft| draft.reference_count_offset = draft.byte_offset,
        |draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::located(Vec::new());
        },
        |draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![1]);
        },
        |draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::located(vec![
                crate::records::identity::Located {
                    value: 1,
                    offset: 15,
                },
            ]);
        },
        |draft| draft.kind_offset += 1,
    ];
    for edit in invalid {
        let mut draft = scope.clone().into_draft();
        edit(&mut draft);
        assert!(DesignParameterScope::try_new(draft).is_err());
        let mut edited = scope.clone();
        assert!(edited.try_edit(edit).is_err());
        assert_eq!(edited, scope);
    }
    let pointers = crate::records::source_offsets::collect(&mut scope.clone());
    let located = |wire: serde_json::Value,
                   pointers: &std::collections::BTreeMap<String, u64>,
                   frame_length: u64| {
        let mut located: DesignParameterScope = serde_json::from_value(wire).unwrap();
        crate::records::source_offsets::restore(&mut located, pointers);
        located.restore_frame_length(frame_length);
        located
            .located_draft()
            .is_some_and(|draft| draft.check_frame_layout().is_ok())
    };
    let wire = serde_json::to_value(&scope).unwrap();
    assert!(located(wire.clone(), &pointers, 128));
    assert!(!located(wire.clone(), &pointers, 89));
    for (field, value) in [
        ("paired_byte_offset", 129),
        ("kind_offset", 0),
        ("feature_ordinal_offset", 49),
        ("previous_history_state_id_offset", 1),
        ("reference_count_offset", 0),
        ("reference_member_offsets.0", 15),
    ] {
        let mut pointers = pointers.clone();
        pointers.insert(field.to_owned(), value);
        assert!(!located(wire.clone(), &pointers, 128), "{field}");
    }
    let mut previous = wire.clone();
    previous["previous_history_state_id"] = serde_json::json!(1);
    assert!(
        !located(previous, &pointers, 128),
        "previous_history_state_id"
    );
    let decoded: DesignParameterScope = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
}

#[test]
fn parameter_scope_located_absent_history_states_remain_legal() {
    let mut scope = DesignParameterScope::empty("scope", DesignFeatureKind::Sketch, 1);
    scope
        .try_edit(|draft| {
            draft.frame_length = 125;
            draft.paired_byte_offset = 125;
            draft.previous_history_state_id_offset = Some(79);
        })
        .unwrap();
    let wire = serde_json::to_value(&scope).unwrap();
    let mut decoded: DesignParameterScope = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&decoded).unwrap(), wire);
    let pointers = crate::records::source_offsets::collect(&mut scope.clone());
    assert_eq!(pointers["previous_history_state_id_offset"], 79);
    assert!(crate::records::source_offsets::restore(&mut decoded, &pointers).is_empty());
    decoded.restore_frame_length(125);
    assert_eq!(decoded, scope);
    assert!(decoded
        .located_draft()
        .is_some_and(|draft| draft.check_frame_layout().is_ok()));
}

#[test]
fn vertex_frame_rejects_displaced_indices_and_recipe_prefix() {
    use crate::records::feature::work_geometry::{DesignVertexRecipe, DesignVertexRecipeDraft};
    let wire = serde_json::json!({
        "record_index": 2, "class_tag": "369", "paired_class_tag": "261",
        "recipe_record_index": 5, "recipe_id": "vertex",
        "recipe_prefix_bytes": "", "recipe_references": [],
        "recipe_program": [0], "next_record_index": 7
    });
    let admitted: DesignVertexRecipe = serde_json::from_value(wire.clone()).unwrap();
    let draft = DesignVertexRecipeDraft {
        record_index: 2,
        byte_offset: 10,
        class_tag: "369".to_owned().try_into().unwrap(),
        paired_byte_offset: 20,
        paired_class_tag: "261".to_owned().try_into().unwrap(),
        recipe_record_index: 5,
        recipe_record_byte_offset: 30,
        recipe_id: "vertex".into(),
        recipe_prefix_offset: 41,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_program_offset: 43,
        recipe_program: vec![0],
        resolution: None,
        next_record_index: 7,
        next_byte_offset: 50,
    };
    let located = DesignVertexRecipe::try_new(draft.clone()).unwrap();
    assert_eq!(serde_json::to_value(&located).unwrap(), wire);
    assert_eq!(serde_json::to_value(&admitted).unwrap(), wire);
    let mut invalid = draft.clone();
    invalid.record_index = u32::MAX;
    assert!(DesignVertexRecipe::try_new(invalid).is_err());
    let mut invalid = draft.clone();
    invalid.recipe_prefix_offset += 1;
    assert!(DesignVertexRecipe::try_new(invalid).is_err());
    for field in ["recipe_record_index", "next_record_index"] {
        let mut invalid = wire.clone();
        invalid[field] = (wire[field].as_u64().unwrap() + 1).into();
        assert!(serde_json::from_value::<DesignVertexRecipe>(invalid).is_err());
    }
    let mut invalid = draft;
    invalid.paired_byte_offset = 10;
    assert!(DesignVertexRecipe::try_new(invalid).is_err());
}

mod three_point_planes;

mod work_points;

/// A Circular Pattern construction around `axis`.
fn circular_construction(
    axis: crate::records::feature::patterns::DesignCircularPatternAxis,
) -> crate::records::feature::patterns::DesignCircularPatternConstruction {
    crate::records::feature::patterns::DesignCircularPatternConstruction {
        count: std::num::NonZeroU32::new(3).expect("checked fixture value"),
        count_record_index: 4,
        count_offset: None,
        angle: cadmpeg_ir::scalar::PositiveAngle::new(1.0).expect("checked fixture value"),
        angle_record_index: 5,
        angle_offset: None,
        axis,
        axis_record_index: 6,
        selection_record_index: 7,
    }
}

mod base_feature_rows;
