// SPDX-License-Identifier: Apache-2.0
//! Synthesis writes supported siblings from neutral content after local loss.

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{DesignParameter, ParameterValue};
use cadmpeg_ir::report::{export::WritePath, Severity};

use crate::F3dCodec;

fn parameter(name: &str, ordinal: u32, value: ParameterValue) -> DesignParameter {
    DesignParameter {
        driving: true,
        id: format!("authored:model:parameter#{ordinal}")
            .try_into()
            .expect("id"),
        owner: None,
        ordinal,
        name: name.into(),
        expression: name.into(),
        display: None,
        value: Some(value),

        properties: Default::default(),
        pmi: None,
        native_ref: None,
    }
}

#[test]
fn a_document_parameter_is_authored_without_native_records_and_keeps_supported_siblings() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    ir.model.parameters = vec![
        parameter(
            "width",
            0,
            ParameterValue::Length(cadmpeg_ir::scalar::Length::new(42.0).expect("length")),
        ),
        parameter("label", 1, ParameterValue::String("omitted string".into())),
        parameter(
            "angle",
            2,
            ParameterValue::Angle(cadmpeg_ir::scalar::Angle::new(1.25).expect("angle")),
        ),
    ];
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("unsupported parameter does not refuse the document");
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    assert!(plan
        .report()
        .losses
        .iter()
        .any(
            |loss| loss.code.local_code() == "writer.design-records-omitted"
                && loss.severity == Severity::Error
        ));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode");
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    let parameters = &decoded.ir().model.parameters;
    assert_eq!(parameters.len(), 2);
    assert_eq!(parameters[0].name, "width");
    assert_eq!(parameters[0].value, ir.model.parameters[0].value);
    assert_eq!(parameters[1].name, "angle");
    assert_eq!(parameters[1].value, ir.model.parameters[2].value);
}

#[test]
fn direct_opacity_reduces_color_without_dropping_geometry() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    ir.model.bodies[0].color = Some(cadmpeg_ir::topology::Color::from_rgba8(10, 20, 30, 100));
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("plan");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "writer.appearance-properties-omitted"));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode");
    assert_eq!(decoded.ir().model.faces.len(), ir.model.faces.len());
    assert_eq!(
        decoded.ir().model.bodies[0]
            .color
            .and_then(|color| color.with_alpha(1.0)),
        ir.model.bodies[0]
            .color
            .and_then(|color| color.with_alpha(1.0))
    );
}

#[test]
fn an_unsupported_face_keeps_the_other_faces_as_an_open_shell() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    let surface = ir.model.faces[0].surface.clone();
    ir.model
        .surfaces
        .iter_mut()
        .find(|record| record.id == surface)
        .unwrap()
        .geometry = cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Unknown { record: None },
    );
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("an unsupported face does not refuse the body");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "writer.geometry-omitted"
            && loss.severity == Severity::Error));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write open shell");
    let output = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("read open shell");
    assert_eq!(output.ir().model.bodies.len(), 1);
    assert_eq!(output.ir().model.faces.len(), 5);
    assert_eq!(
        output.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    assert!(
        cadmpeg_ir::validate::validate_neutral(output.ir(), Vec::new())
            .unwrap()
            .is_ok()
    );
}

#[test]
fn neutral_configurations_and_parameters_are_authored_together() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    ir.model.parameters.push(parameter(
        "width",
        0,
        ParameterValue::Length(cadmpeg_ir::scalar::Length::new(12.0).unwrap()),
    ));
    ir.model
        .configurations
        .push(cadmpeg_ir::features::DesignConfiguration {
            id: "authored:model:configuration#0".try_into().unwrap(),
            ordinal: 0,
            active: true,
            source_index: None,
            name: Some("Wide".into()),
            material: Some("Steel".into()),
            properties: [(
                cadmpeg_core::text::NonBlankString::new("activation_rule:0").unwrap(),
                "width > 10".into(),
            )]
            .into(),
            parameter_overrides: [(ir.model.parameters[0].id.clone(), "24 mm".into())].into(),
            bodies: None,
            parameter_values: Default::default(),
            feature_states: Default::default(),
            native_ref: None,
        });
    let mut bytes = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("author configuration archive");
    let output = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("read configuration archive");
    assert_eq!(output.ir().model.bodies.len(), 1);
    assert_eq!(output.ir().model.parameters.len(), 1);
    assert_eq!(
        ir.model.parameters[0].value,
        Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(12.0).unwrap()
        ))
    );
    let configuration = &output.ir().model.configurations[0];
    assert_eq!(configuration.name.as_deref(), Some("Wide"));
    assert_eq!(configuration.material.as_deref(), Some("Steel"));
    assert!(configuration.active);
    assert!(configuration.parameter_overrides.is_empty());
    let parameter = &output.ir().model.parameters[0];
    assert_eq!(parameter.name, "width");
    let native = crate::test_support::native_test::f3d_native(output.ir());
    let record = native
        .design_parameters
        .iter()
        .find(|record| record.name() == "width")
        .unwrap();
    assert_eq!(parameter.id, crate::ids::neutral_parameter_id(record));
    assert_eq!(
        parameter.expression.tokens,
        vec![cadmpeg_ir::features::ExpressionToken::Text("24 mm".into())]
    );
    assert_eq!(
        parameter.expression.render_with(|_| None).as_deref(),
        Some("24 mm")
    );
    assert_eq!(
        parameter
            .expression
            .literal_value_as(&ParameterValue::Length(
                cadmpeg_ir::scalar::Length::new(12.0).unwrap()
            )),
        Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(24.0).unwrap()
        ))
    );
    assert_eq!(
        parameter.value,
        Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(24.0).unwrap()
        ))
    );
    assert!(
        cadmpeg_ir::validate::validate_neutral(output.ir(), Vec::new())
            .unwrap()
            .is_ok()
    );
    assert!(configuration
        .properties
        .values()
        .any(|value| value == "width > 10"));
}

#[test]
fn source_free_parameter_without_cache_evaluates_only_its_current_literal() {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut width = parameter(
        "width",
        0,
        ParameterValue::Length(cadmpeg_ir::scalar::Length::new(24.0).unwrap()),
    );
    width.expression = cadmpeg_ir::features::Expression::text("24 mm");
    width.value = None;
    ir.model.parameters.push(width);
    assert!(ir.native.namespace("f3d").is_none());
    let mut bytes = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap()
        .write_to(&mut bytes)
        .unwrap();
    assert!(ir.model.parameters[0].value.is_none());
    let output = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.parameters.len(), 1);
    assert_eq!(output.ir().model.parameters[0].name, "width");
    assert_eq!(
        output.ir().model.parameters[0].expression,
        cadmpeg_ir::features::Expression::text("24 mm")
    );
    assert_eq!(
        output.ir().model.parameters[0].value,
        Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(24.0).unwrap()
        ))
    );
    assert!(
        cadmpeg_ir::validate::validate_neutral(output.ir(), Vec::new())
            .unwrap()
            .is_ok()
    );
}

#[test]
fn an_authored_empty_document_writes_a_valid_archive_and_reports_empty_geometry() {
    let ir = cadmpeg_ir::CadIr::empty();
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("empty documents have a valid F3D representation");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.taxonomy()
            == cadmpeg_ir::report::loss::LossTaxonomy::NoExportableSolids));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write empty archive");
    for depth in [
        cadmpeg_core::decode::DecodeDepth::Geometry,
        cadmpeg_core::decode::DecodeDepth::Full,
    ] {
        let output = F3dCodec
            .decode(
                &mut Cursor::new(&bytes),
                &DecodeOptions {
                    depth,
                    ..DecodeOptions::default()
                },
            )
            .expect("read empty archive");
        assert!(output.ir().model.bodies.is_empty());
        assert!(!output.report().geometry_transferred());
        assert_eq!(output.ir().scope, depth.required_scope());
        assert!(output.source_fidelity().unrepresented().is_empty());
        assert!(!output.report().losses.iter().any(|loss| matches!(
            loss.code.local_code(),
            "geometry.brep-not-transferred" | "topology.graph-not-built" | "depth.not-delivered"
        )));
    }
}

fn history(ordinal: u64) -> crate::history_records::AsmHistory {
    use crate::history_records::{AsmDeltaState, AsmHistory};
    let id = format!("f3d:authored:asm-history#{ordinal}");
    AsmHistory::new(
        id.clone(),
        ordinal,
        Some(crate::history_records::AsmPreamble {
            stream_size: 1,
            history_entry_count: 1,
        }),
        false,
        vec![AsmDeltaState {
            id: format!("f3d:authored/history-{ordinal}:delta-state#0"),
            parent: id,
            byte_offset: Some(0),
            state_id: 1,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 1,
            partner_ref: None,
            owner_ref: -1,
            bulletin_boards: vec![],
            records: vec![],
            transition: None,
        }],
    )
}

#[test]
fn an_empty_model_history_is_unrepresented_when_design_is_not_projected() {
    use crate::test_support::native_test::{f3d_native, f3d_native_mut};
    let mut ir = cadmpeg_ir::CadIr::empty();
    f3d_native_mut(&mut ir).asm_histories = vec![history(0)];
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("typed empty history is supported");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write typed history");
    let full = F3dCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("read typed history");
    assert_eq!(f3d_native(full.ir()).asm_histories.len(), 1);
    assert!(full.source_fidelity().unrepresented().is_empty());
    let geometry = F3dCodec
        .decode(
            &mut Cursor::new(&bytes),
            &DecodeOptions {
                depth: cadmpeg_core::decode::DecodeDepth::Geometry,
                ..DecodeOptions::default()
            },
        )
        .expect("read empty model without history projection");
    assert!(geometry.ir().scope.model);
    assert!(!geometry.report().geometry_transferred());
    assert!(!geometry.ir().scope.design);
    assert!(f3d_native(geometry.ir()).asm_histories.is_empty());
    assert!(geometry
        .source_fidelity()
        .unrepresented()
        .contains(&cadmpeg_ir::document::ArenaClass::Design));
}

#[test]
fn unsupported_typed_histories_reduce_without_losing_neutral_geometry() {
    use crate::test_support::native_test::{f3d_native, f3d_native_mut};
    let mut projected = history(0);
    projected.states_mut()[0].transition = Some(crate::history_records::AsmHistoricalTransition {
        previous_state_id: None,
        records: crate::history_records::AsmHistoricalEntityDelta {
            inserted: vec![3],
            ..Default::default()
        },
        topology: Default::default(),
        record_revisions: vec![],
        rows: Default::default(),
    });
    let mut external_owner = history(0);
    external_owner.states_mut()[0].owner_ref = 7;
    for (histories, expected) in [
        (vec![history(0), history(1)], 1),
        (vec![external_owner], 0),
        (vec![projected], 0),
        (
            {
                let mut record = history(0);
                record.binding_budget_exceeded = true;
                vec![record]
            },
            0,
        ),
    ] {
        let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
        let history_ids = histories
            .iter()
            .map(|history| history.id.clone())
            .collect::<Vec<_>>();
        f3d_native_mut(&mut ir).asm_histories = histories;
        let (output, report) = super::source_less_analytic::synthesize_document(&ir, None);
        assert_eq!(output.ir().model.bodies.len(), 1);
        assert_eq!(output.ir().model.faces.len(), ir.model.faces.len());
        assert_eq!(f3d_native(output.ir()).asm_histories.len(), expected);
        for (ordinal, id) in history_ids.iter().enumerate() {
            assert_eq!(
                report
                    .native_dispositions
                    .record_disposition("f3d", "asm_histories", id),
                if ordinal < expected {
                    cadmpeg_ir::codec::write::NativeDisposition::Written
                } else {
                    cadmpeg_ir::codec::write::NativeDisposition::Dropped
                },
                "original history {id}",
            );
        }
        assert!(report.losses.iter().any(|loss| {
            matches!(
                loss.code.local_code(),
                "writer.native-records-omitted" | "native.records-omitted"
            )
        }));
    }
}

#[test]
fn admitted_relaxed_native_type_identifiers_reemit_without_omission() {
    use crate::records::entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData};
    use crate::records::identity::ReferenceRun;
    use crate::test_support::native_test::{f3d_native, f3d_native_mut};
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    f3d_native_mut(&mut ir).design_types = [
        "11111111-2222-3333-4444-555555555555",
        "____________________________________",
    ]
    .iter()
    .enumerate()
    .map(|(ordinal, guid)| {
        SegmentType::try_new(
            format!("f3d:authored/MetaStream.dat:design-type#{ordinal}"),
            SegmentTypeData {
                class_tag: (256u32 + u32::try_from(ordinal).expect("fixture ordinal"))
                    .to_string()
                    .try_into()
                    .expect("fixture class tag"),
                byte_offset: Some(u64::try_from(ordinal).expect("ordinal")),
                type_guid: (*guid)
                    .to_owned()
                    .try_into()
                    .expect("reader type identifier"),
                type_guid_offset: Some(0),
                base_type_guid: BaseTypeGuid::Absent,
                version: 1,
                version_offset: Some(0),
                module: "Vendor".into(),
                entities: ReferenceRun::unlocated(vec![]),
            },
        )
        .expect("typed registration")
    })
    .collect();
    let (output, report) = super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(output.ir().model.bodies.len(), 1);
    assert_eq!(output.ir().model.faces.len(), ir.model.faces.len());
    assert!(f3d_native(output.ir())
        .design_types
        .iter()
        .any(|record| { record.type_guid.as_str() == "11111111-2222-3333-4444-555555555555" }));
    assert!(f3d_native(output.ir())
        .design_types
        .iter()
        .any(|record| { record.type_guid.as_str() == "____________________________________" }));
    assert!(!report.losses.iter().any(|loss| {
        loss.code.local_code() == "writer.native-records-omitted"
            && loss.message.contains("design_types")
    }));
}

#[test]
fn independent_native_streams_may_reuse_local_registry_and_recipe_identities() {
    use crate::records::entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData};
    use crate::records::identity::{RecordedValue, ReferenceRun};
    use crate::records::recipes::{
        ConstructionRecipe, ConstructionRecipeDesign, ConstructionRecipeKind,
    };
    use crate::test_support::native_test::{f3d_native, f3d_native_mut};
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    {
        let mut native = f3d_native_mut(&mut ir);
        for stream in ["first", "second"] {
            native.design_types.push(
                SegmentType::try_new(
                    format!("f3d:{stream}/MetaStream.dat:design-type#0"),
                    SegmentTypeData {
                        class_tag: (256u32).to_string().try_into().expect("fixture class tag"),
                        byte_offset: Some(0),
                        type_guid: "11111111-2222-3333-4444-555555555555"
                            .to_owned()
                            .try_into()
                            .expect("guid"),
                        type_guid_offset: Some(0),
                        base_type_guid: BaseTypeGuid::Absent,
                        version: 1,
                        version_offset: Some(0),
                        module: "Vendor".into(),
                        entities: ReferenceRun::unlocated(vec![]),
                    },
                )
                .expect("typed registration"),
            );
            native.construction_recipes.push(ConstructionRecipe {
                id: format!("f3d:{stream}/BulkStream.dat:recipe#0"),
                byte_offset: Some(0),
                kind: ConstructionRecipeKind::Body,
                design: Some(ConstructionRecipeDesign {
                    id: RecordedValue {
                        value: "333".into(),
                        offset: Some(0),
                    },
                    selector: None,
                }),
                recipe_index: 0,
                record_index: Some(RecordedValue {
                    value: 777,
                    offset: Some(0),
                }),
            });
        }
    }
    let (output, report) = super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(output.ir().model.bodies.len(), 1);
    assert_eq!(output.ir().model.faces.len(), ir.model.faces.len());
    assert_eq!(f3d_native(output.ir()).construction_recipes.len(), 1);
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "writer.native-records-omitted"));
}

#[test]
fn independent_body_keys_and_browser_suffixes_are_reminted_without_geometry_loss() {
    use crate::records::bodies::{BodyVisibility, BodyVisibilityWire};
    use crate::test_support::native_test::{f3d_native, f3d_native_mut};
    use cadmpeg_asm::brep::records::{identity::NativeRecordNamespace, BodyNativeKey};
    let first = cadmpeg_ir::examples::unit_cube().expect("cube");
    let text = first
        .to_canonical_json()
        .expect("json")
        .replace("synthetic:cube:", "synthetic:second:");
    let mut second = cadmpeg_ir::CadIr::from_json(&text).expect("second cube");
    let mut original = first;
    macro_rules! append { ($($field:ident),*) => { $(original.model.$field.append(&mut second.model.$field);)* }; }
    append!(
        bodies, regions, shells, faces, loops, coedges, edges, vertices, points, surfaces, curves
    );
    original.model.bodies[0].visible = Some(true);
    original.model.bodies[1].visible = Some(false);
    for missing_first in [false, true] {
        let mut ir = original.clone();
        {
            let bodies = ir.model.bodies.clone();
            let mut native = f3d_native_mut(&mut ir);
            for (ordinal, body) in bodies.iter().enumerate() {
                let key = if missing_first { 1 } else { 42 };
                if !missing_first || ordinal == 1 {
                    native.body_native_keys.push(BodyNativeKey {
                        source_namespace: NativeRecordNamespace::new(cadmpeg_asm::asm_format!(
                            "f3d"
                        )),
                        record_index: u32::try_from(ordinal).expect("ordinal"),
                        body: body.id.clone(),
                        body_ordinal: u32::try_from(ordinal).expect("ordinal"),
                        source_brep: None,
                        asm_body_key: Some(key),
                    });
                }
                native.body_visibilities.push(
                    BodyVisibility::try_from(BodyVisibilityWire {
                        id: format!("f3d:part{ordinal}/BulkStream.dat:body-visibility#{key}"),
                        body: body.id.clone(),
                        stream: format!("part{ordinal}/BulkStream.dat"),
                        byte_offset: 0,
                        asm_body_key_offset: 0,
                        asm_body_key: key,
                        entity_suffix: 84,
                        visible: body.visible.expect("visibility"),
                    })
                    .expect("typed browser visibility"),
                );
            }
        }
        let (output, _) = super::source_less_analytic::synthesize_document(&ir, None);
        assert_eq!(output.ir().model.bodies.len(), 2);
        assert_eq!(output.ir().model.faces.len(), 12);
        assert_eq!(
            output
                .ir()
                .model
                .bodies
                .iter()
                .map(|body| body.visible)
                .collect::<Vec<_>>(),
            vec![Some(true), Some(false)]
        );
        let native = f3d_native(output.ir());
        let keys = native
            .body_native_keys
            .iter()
            .filter_map(|record| record.asm_body_key)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&if missing_first { 1 } else { 42 }));
        assert_eq!(
            native
                .body_visibilities
                .iter()
                .map(|record| record.entity_suffix)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            2
        );
    }
}

#[test]
fn an_unprojected_native_record_header_does_not_refuse_unrelated_geometry() {
    use crate::records::decal::DesignRecordHeader;
    use crate::test_support::native_test::{f3d_native, f3d_native_mut};
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube");
    f3d_native_mut(&mut ir).design_record_headers = vec![DesignRecordHeader {
        id: "f3d:authored/BulkStream.dat:design-record-header#0".into(),
        record_index: 123,
        class_tag: "256".to_owned().try_into().expect("class"),
        byte_offset: None,
    }];
    let (output, report) = super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(output.ir().model.bodies.len(), 1);
    assert_eq!(output.ir().model.faces.len(), ir.model.faces.len());
    assert!(f3d_native(output.ir()).design_record_headers.is_empty());
    assert_eq!(
        report
            .native_dispositions
            .disposition("f3d", "design_record_headers"),
        cadmpeg_ir::codec::write::NativeDisposition::Dropped
    );
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "native.records-omitted"));
}

#[test]
fn historical_bulletin_slots_are_omitted_when_their_revisions_have_no_typed_writer() {
    use crate::test_support::native_test::{f3d_native, f3d_native_mut};
    let source = crate::test_support::zip_test::f3d_with_smbh(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_with_history_smbh(),
    );
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("public history fixture");
    let mut ir = decoded.ir().clone();
    {
        let mut native = f3d_native_mut(&mut ir);
        assert!(native
            .asm_histories
            .iter()
            .flat_map(|history| history.states())
            .any(|state| !state.bulletin_boards.is_empty()));
        for state in native
            .asm_histories
            .iter_mut()
            .flat_map(|history| history.states_mut())
        {
            state.records.clear();
            state.transition = None;
        }
    }
    let (output, report) = super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(output.ir().model.bodies.len(), ir.model.bodies.len());
    assert_eq!(output.ir().model.faces.len(), ir.model.faces.len());
    assert!(f3d_native(output.ir()).asm_histories.is_empty());
    assert_eq!(
        report
            .native_dispositions
            .disposition("f3d", "asm_histories"),
        cadmpeg_ir::codec::write::NativeDisposition::Dropped
    );
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "native.records-omitted"));
}
