// SPDX-License-Identifier: Apache-2.0
//! Writer harness wiring for SolidWorks (#237 item, #217 §5.0, #232 §3.3 and §5.2).
//!
//! Injected-error, capacity-helper and `max_entities` runners from
//! `cadmpeg-test-support`; the residual checker over the §3.3 statement list;
//! and the `REDUCED_FIELDS` row runner.

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::residual::{residual_findings, ResidualSpec};
use cadmpeg_test_support::robustness::{
    injected_error_findings, max_entities_findings, standard_injections,
};

use std::io::Cursor;

use crate::SldprtCodec;

#[test]
fn encoder_exposes_its_reduced_fields() {
    assert_eq!(SldprtCodec.reduced_fields(), crate::writer::REDUCED_FIELDS);
}

fn seed_golden() -> cadmpeg_ir::CadIr {
    let harness = cadmpeg_test_support::golden::Harness::new(
        env!("CARGO_MANIFEST_DIR"),
        "sldprt",
        "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-sldprt golden",
    );
    let (_, bytes) = harness
        .fixture_inputs()
        .into_iter()
        .find(|(name, _)| name == "authored_complete_cylinder")
        .expect("commit the robustness seed fixture");
    SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the robustness seed")
        .ir()
        .clone()
}

fn request() -> TargetRequest<'static> {
    TargetRequest::Inherit
}

#[test]
fn vertex_tolerance_underflow_preserves_the_vertex_with_a_named_loss() {
    let original = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut source = Vec::new();
    SldprtCodec
        .plan(EncodeInput::new(&original, None), request())
        .unwrap()
        .write_to(&mut source)
        .unwrap();
    let source = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = source.ir().clone();
    let vertex_id = ir.model.vertices[0].id.to_string();
    let point_id = ir.model.vertices[0].point.clone();
    let position = ir
        .model
        .points
        .iter()
        .find(|point| point.id == point_id)
        .unwrap()
        .position();
    ir.model.vertices[0].tolerance = cadmpeg_ir::scalar::PositiveReal::new(f64::from_bits(1));
    for fidelity in [None, Some(source.source_fidelity())] {
        let plan = SldprtCodec
            .plan(EncodeInput::new(&ir, fidelity), request())
            .unwrap();
        assert!(plan
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains(&vertex_id) && loss.message.contains("tolerance")));
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(decoded.ir().model.vertices.len(), ir.model.vertices.len());
        assert!(decoded
            .ir()
            .model
            .vertices
            .iter()
            .any(|vertex| vertex.tolerance.is_none()
                && decoded
                    .ir()
                    .model
                    .points
                    .iter()
                    .any(|point| point.id == vertex.point && point.position() == position)));
    }
}

#[test]
fn injected_errors_succeed_and_name_the_record() {
    let seed = seed_golden();
    // The recompute-flag injection flips `needs_recompute` to true, which is
    // legal stored state (#209 §2.5), so it never invalidates the seed and is
    // inapplicable here; the remaining standard injections run.
    let injections: Vec<_> = standard_injections()
        .into_iter()
        .filter(|injection| injection.name != "recompute-flag")
        .collect();
    let findings = injected_error_findings(&seed, &SldprtCodec, request(), &injections);
    assert!(
        findings.is_empty(),
        "injected-error findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn material_name_capacity_is_a_typed_target_refusal() {
    let source = crate::test_support::appearance::sldprt_with_body_and_material(
        &crate::test_support::parasolid::triangle_body(),
        "Material",
        [0, 0, 0],
    );
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    ir.model.appearances[0].name = Some("n".repeat(256));
    for fidelity in [None, Some(decoded.source_fidelity())] {
        let error = SldprtCodec
            .plan(EncodeInput::new(&ir, fidelity), request())
            .err()
            .expect("material name exceeds target width");
        assert!(
            matches!(error, cadmpeg_ir::codec::write::EncodeError::TargetCapacity { field, limit: 255, value: 256 } if field.field == "material.name")
        );
    }
}

#[test]
fn display_descriptor_reports_item_count_capacity() {
    let mut bytes = Vec::new();
    let count = usize::try_from(u64::from(u32::MAX) + 1).unwrap();
    let error: cadmpeg_ir::codec::write::EncodeError =
        crate::writer::descriptor(&mut bytes, 4, 8, 2, count, &[])
            .unwrap_err()
            .into();
    assert!(bytes.is_empty());
    assert!(
        matches!(error, cadmpeg_ir::codec::write::EncodeError::TargetCapacity {
        field, limit: 4_294_967_295, value: 4_294_967_296
    } if field.field == "display_list.channel_item_count")
    );
}

#[test]
fn feature_name_capacity_reaches_the_encoder() {
    let mut source = crate::test_support::container::sldprt_with_body(
        &crate::test_support::parasolid::triangle_body(),
    );
    source.extend(crate::test_support::container::make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="MatrizL1" Type="MatrizL" id="132"><Dimension Name="D1">15</Dimension></Feature></Keywords>"#,
    ));
    source.extend(crate::test_support::container::make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &crate::test_support::history::resolved_feature_classes_with_ids(&[(
            "moLPattern_c",
            "MatrizL1",
            132,
        )]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    let native = crate::test_support::native::sldprt_native(decoded.ir());
    let id = native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .find(|feature| feature.input_class.is_some())
        .expect("fixture has a native feature-input class")
        .id
        .clone();
    ir.model
        .features
        .iter_mut()
        .find(|feature| feature.native_ref.as_deref() == Some(id.as_str()))
        .expect("native feature has a neutral projection")
        .name = Some("😀".repeat(128));
    for fidelity in [None, Some(decoded.source_fidelity())] {
        let error = SldprtCodec
            .plan(EncodeInput::new(&ir, fidelity), request())
            .err()
            .expect("feature name exceeds target width");
        assert!(matches!(error,
            cadmpeg_ir::codec::write::EncodeError::TargetCapacity {
                field, limit: 255, value: 256,
            } if field.field == "feature_input.name_length"
        ));
    }
}

#[test]
fn parasolid_header_reports_field_capacity() {
    let header =
        crate::writer::parasolid_stream_named(&[], &"s".repeat(255), &"d".repeat(65_535)).unwrap();
    assert_eq!(&header[4..6], &u16::MAX.to_be_bytes());
    assert_eq!(header[65_543], u8::MAX);
    for (schema, description, field, limit, value) in [
        (
            "schema".to_owned(),
            "d".repeat(65_536),
            "parasolid.description_length",
            65_535,
            65_536,
        ),
        (
            "s".repeat(256),
            "description".to_owned(),
            "parasolid.schema_length",
            255,
            256,
        ),
    ] {
        let error: cadmpeg_ir::codec::write::EncodeError =
            crate::writer::parasolid_stream_named(&[], &schema, &description)
                .unwrap_err()
                .into();
        assert!(
            matches!(error, cadmpeg_ir::codec::write::EncodeError::TargetCapacity {
            field: actual, limit: actual_limit, value: actual_value,
        } if actual.field == field && actual_limit == limit && actual_value == value)
        );
    }
}

#[test]
fn capacity_helpers_follow_their_rows() {
    for field in crate::writer::CAPACITY_FIELDS
        .iter()
        .filter(|field| field.helper == "field_u16")
    {
        assert!(crate::writer::field_u16(field, field.limit).is_ok());
        let error: cadmpeg_ir::codec::write::EncodeError =
            crate::writer::field_u16(field, field.limit + 1)
                .unwrap_err()
                .into();
        assert!(
            matches!(error, cadmpeg_ir::codec::write::EncodeError::TargetCapacity {
            field: actual, limit, value
        } if actual.field == field.field && limit == field.limit && value == field.limit + 1)
        );
    }
    assert_eq!(
        crate::writer::display_list_c_value(2_147_483_648).unwrap(),
        4_294_967_294
    );
    let error: cadmpeg_ir::codec::write::EncodeError =
        crate::writer::display_list_c_value(2_147_483_649)
            .unwrap_err()
            .into();
    assert!(
        matches!(error, cadmpeg_ir::codec::write::EncodeError::TargetCapacity {
        field, limit: 4_294_967_295, value: 4_294_967_296
    } if field.field == "display_list.list_c_value")
    );
    assert_eq!(crate::writer::unit_name_byte_len(127).unwrap(), 254);
    let error: cadmpeg_ir::codec::write::EncodeError =
        crate::writer::unit_name_byte_len(128).unwrap_err().into();
    assert!(
        matches!(error, cadmpeg_ir::codec::write::EncodeError::TargetCapacity {
        field, limit: 255, value: 256
    } if field.field == "document.unit_name_byte_length")
    );
    assert_eq!(crate::writer::feature_input_name_len(255).unwrap(), 255);
    let error: cadmpeg_ir::codec::write::EncodeError = crate::writer::feature_input_name_len(256)
        .unwrap_err()
        .into();
    assert!(
        matches!(error, cadmpeg_ir::codec::write::EncodeError::TargetCapacity {
        field, limit: 255, value: 256
    } if field.field == "feature_input.name_length")
    );
    let name = "n".repeat(crate::writer::CAPACITY_FIELDS[0].limit as usize);
    assert!(crate::writer::material_name_len(&name).is_ok());
    let overlong = "n".repeat(crate::writer::CAPACITY_FIELDS[0].limit as usize + 1);
    assert!(crate::writer::material_name_len(&overlong).is_err());
    let limit = crate::writer::CAPACITY_FIELDS[1].limit;
    let index = usize::try_from(limit).expect("type-id limit fits usize");
    assert!(crate::writer::section_type_id_at(index).is_ok());
    let error: cadmpeg_ir::codec::write::EncodeError = crate::writer::section_type_id_at(index + 1)
        .unwrap_err()
        .into();
    assert!(
        matches!(error, cadmpeg_ir::codec::write::EncodeError::TargetCapacity { field, limit: actual_limit, value } if field.field == "section.type_id" && actual_limit == limit && value == limit + 1)
    );
    for field in crate::writer::CAPACITY_FIELDS
        .iter()
        .filter(|field| field.helper == "field_u32")
    {
        let limit = usize::try_from(field.limit).expect("section length limit fits usize");
        assert!(crate::writer::field_u32(field, limit).is_ok());
        let error: cadmpeg_ir::codec::write::EncodeError =
            crate::writer::field_u32(field, limit + 1)
                .unwrap_err()
                .into();
        assert!(
            matches!(error, cadmpeg_ir::codec::write::EncodeError::TargetCapacity {
            field: actual, limit: actual_limit, value
        } if actual.field == field.field && actual_limit == field.limit && value == field.limit + 1)
        );
    }
}

#[test]
fn synthesized_unit_name_reports_its_byte_capacity() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};

    let mut ir = crate::test_support::ir::source_less_cube();
    ir.model.attributes.push(SourceAttribute {
        native_ref: None,
        id: "sldprt:metadata:source_linear_unit_name#0:0"
            .try_into()
            .unwrap(),
        target: AttributeTarget::Document,
        name: "source_linear_unit_name".into(),
        values: vec![Some(AttributeValue::String("u".repeat(128)))],
    });
    let error: cadmpeg_ir::codec::write::EncodeError =
        crate::writer::metadata_payloads(&ir, None, 0.001)
            .unwrap_err()
            .into();
    assert!(
        matches!(error, cadmpeg_ir::codec::write::EncodeError::TargetCapacity {
        field, limit: 255, value: 256
    } if field.field == "document.unit_name_byte_length")
    );
    let error = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .err()
        .expect("the unit name exceeds the native byte count");
    assert!(
        matches!(error, cadmpeg_ir::codec::write::EncodeError::TargetCapacity {
        field, limit: 255, value: 256
    } if field.field == "document.unit_name_byte_length")
    );
}

#[test]
fn max_entities_refuses_an_over_ceiling_plan() {
    let seed = seed_golden();
    let count = u64::try_from(seed.model.entity_count()).expect("entity count fits");
    let findings = max_entities_findings(&seed, &SldprtCodec, request(), count);
    assert!(
        findings.is_empty(),
        "max_entities findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn reduced_rows_hold_their_storage() {
    // The declared rows feed the cross-closure comparison, which applies
    // them positionally. The probe harness addresses records by identity,
    // which synthesis regenerates, so the storage itself is pinned here:
    // colours quantize to 8-bit channels and tessellation scalars to f32.
    let rows = crate::writer::REDUCED_FIELDS;
    assert_eq!(
        rows.iter().map(|row| row.path).collect::<Vec<_>>(),
        [
            "model.tessellations[].vertices[]",
            "model.tessellations[].vertex_normals[]",
            "model.appearances[].base_color",
        ]
    );
    assert_eq!(
        rows.iter().map(|row| row.storage).collect::<Vec<_>>(),
        [
            cadmpeg_ir::codec::write::ReducedStorage::F32,
            cadmpeg_ir::codec::write::ReducedStorage::F32,
            cadmpeg_ir::codec::write::ReducedStorage::U8Channel,
        ]
    );
    let payload = crate::writer::material_payload(
        "probe",
        cadmpeg_ir::topology::Color::new(0.25, 0.5, 0.75, 1.0).expect("probe colour admits"),
    )
    .expect("probe material encodes");
    let tag = b"moVisualProperties_c";
    let at = payload
        .windows(tag.len())
        .position(|window| window == tag)
        .expect("material tag opens the payload");
    assert_eq!(
        payload[at + tag.len()..at + tag.len() + 4],
        [0x40, 0x80, 0xBF, 0x00],
        "8-bit channels quantize 0.25/0.5/0.75 to 64/128/191"
    );
    assert_eq!(
        crate::writer::tessellation_f32(0.1, "probe").expect("f32 admits 0.1"),
        0.1f32,
        "tessellation scalars store as f32"
    );
}

#[test]
fn synthesized_records_carry_their_specified_residuals() {
    let seed = seed_golden();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&seed, None), request())
        .expect("plan the residual seed");
    let mut bytes = Vec::new();
    let report = plan.write_to(&mut bytes).expect("write the residual seed");
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the residual output");
    let after = serde_json::to_value(decoded.ir()).expect("serialize the re-decode");
    let specs = residual_specs(&seed);
    let findings = residual_findings(&after, &report.losses, &specs);
    assert!(
        findings.is_empty(),
        "residual findings:\n{}",
        findings.join("\n")
    );
}

/// Residual positions the SolidWorks writer synthesizes, from `docs/formats/`.
///
/// No residual rule is stated in `docs/formats/sldprt.md` yet; the first
/// stated rule adds its spec here (#232 §3.3).
fn residual_specs(seed: &cadmpeg_ir::CadIr) -> Vec<ResidualSpec> {
    let _ = seed;
    Vec::new()
}

#[test]
fn compressed_sketch_lengths_report_their_target_fields() {
    for field in &crate::writer::CAPACITY_FIELDS[33..36] {
        assert_eq!(
            crate::writer::field_u32(field, usize::try_from(u32::MAX).unwrap()).unwrap(),
            u32::MAX
        );
        let error: cadmpeg_ir::codec::write::EncodeError =
            crate::writer::field_u32(field, usize::try_from(u64::from(u32::MAX) + 1).unwrap())
                .unwrap_err()
                .into();
        assert!(
            matches!(error, cadmpeg_ir::codec::write::EncodeError::TargetCapacity {
            field: actual, limit, value
        } if actual.field == field.field && limit == u64::from(u32::MAX) && value == limit + 1)
        );
    }
}

#[test]
fn configuration_source_index_allocation_rejects_exhaustion() {
    let mut used = std::collections::HashSet::from([u32::MAX]);
    let mut next = u32::MAX;
    let error = crate::writer::reserve_configuration_index(&mut used, &mut next).unwrap_err();
    assert!(
        matches!(error, crate::writer::write_error::WriteError::Capacity { field, value }
        if field.field == "configuration.source_index" && field.limit == u64::from(u32::MAX) && value == u64::from(u32::MAX) + 1)
    );
}

#[test]
fn typed_node_id_reports_the_u32_boundary() {
    let mut next = u64::from(u32::MAX);
    assert_eq!(
        crate::writer::next_typed_node_id(&mut next).unwrap(),
        u32::MAX
    );
    let error = crate::writer::next_typed_node_id(&mut next).unwrap_err();
    assert!(
        matches!(error, crate::writer::write_error::WriteError::Capacity { field, value }
        if field.field == "parasolid.typed_node_id" && field.limit == u64::from(u32::MAX) && value == u64::from(u32::MAX) + 1)
    );
}
