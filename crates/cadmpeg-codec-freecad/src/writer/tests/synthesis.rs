// SPDX-License-Identifier: Apache-2.0
//! Content authored from CADIR never depends on retained archive bytes.

use crate::test_support::test_archive::{CORE_DESIGN_PRODUCT, GEOMETRY};
use crate::FcstdCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

fn synthesized(source: &[u8], target: &str) -> cadmpeg_ir::codec::DecodeResult {
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit(target),
        )
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Synthesized { .. }
    ));
    let report = plan.report().clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    cadmpeg_test_support::contract::assert_written_arenas_agree(&report, decoded.ir(), output.ir());
    crate::test_support::test_archive::assert_valid_document(output.ir());
    assert!(crate::test_support::validate_native(output.ir()).is_empty());
    output
}

#[test]
fn source_less_synthesis_keeps_neutral_brep_and_history() {
    let baseline = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .unwrap();
    let output = synthesized(CORE_DESIGN_PRODUCT, "fcstd:schema-4");
    let before = &baseline.ir().model;
    let after = &output.ir().model;
    assert_eq!(after.bodies.len(), before.bodies.len(), "body count");
    assert_eq!(after.faces.len(), before.faces.len(), "face count");
    assert_eq!(after.edges.len(), before.edges.len(), "edge count");
    assert_eq!(after.vertices.len(), before.vertices.len(), "vertex count");
    assert_eq!(after.features.len(), before.features.len(), "feature count");
    assert_eq!(
        after.sketch_entities.len(),
        before.sketch_entities.len(),
        "sketch geometry count"
    );
    assert_eq!(
        after.sketch_constraints.len(),
        before.sketch_constraints.len(),
        "constraint count"
    );
}

#[test]
fn source_less_tolerance_modes_keep_the_sixteen_authored_history_parameters() {
    use cadmpeg_ir::features::{FeatureDefinition, FuzzyTolerance};
    let source = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/design_history.FCStd"
    ));
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.parameters.len(), 16);
    for mode in [FuzzyTolerance::KernelDefault, FuzzyTolerance::Automatic] {
        let mut current = decoded.ir().clone();
        let mut changed = Vec::new();
        for feature in &mut current.model.features {
            if let FeatureDefinition::PostProcess {
                operation, refine, ..
            } = feature.definition()
            {
                let definition = FeatureDefinition::PostProcess {
                    operation: operation.clone(),
                    refine: *refine,
                    fuzzy_tolerance: mode.clone(),
                };
                feature.evaluation.set_definition(definition);
                changed.push(feature.id.clone());
            }
        }
        assert_eq!(changed.len(), 3);
        let plan = FcstdCodec
            .plan(EncodeInput::new(&current, None), TargetRequest::Inherit)
            .unwrap();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let output = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(output.ir().model.parameters.len(), 16);
        assert!(!output
            .ir()
            .model
            .parameters
            .iter()
            .any(|parameter| parameter.name == "FuzzyTolerance"));
        for id in &changed {
            let feature = output
                .ir()
                .model
                .features
                .iter()
                .find(|feature| &feature.id == id)
                .unwrap();
            assert!(
                matches!(feature.definition(), FeatureDefinition::PostProcess { fuzzy_tolerance, .. } if fuzzy_tolerance == &mode)
            );
        }
    }
}

#[test]
fn forbidden_xml_label_characters_reduce_and_report_instead_of_refusing() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    ir.model.features[0].name = Some("Label\0\u{1f}\t\nwith literal &#xFFFD;".into());
    let body = super::super::synthesis::write(&ir, "4").unwrap();
    assert_eq!(
        body.coverage
            .disposition(cadmpeg_ir::schema::EntityKind::Feature),
        cadmpeg_ir::codec::write::ArenaDisposition::Reported
    );
    let notes: Vec<_> = body
        .losses
        .iter()
        .filter(|loss| loss.message.contains("XML-forbidden"))
        .collect();
    assert_eq!(notes.len(), 1);
    assert!(notes[0].message.contains("Property[Label]/String: 2"));
    assert_eq!(notes[0].code.local_code(), "arena.not-transferred");
    let output = FcstdCodec
        .decode(&mut Cursor::new(body.bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output.ir().model.features.iter().any(|feature| {
        feature.name.as_deref() == Some("Label\u{fffd}\u{fffd}\t\nwith literal &#xFFFD;")
    }));
    assert!(crate::test_support::validate_native(output.ir()).is_empty());
}

#[test]
fn every_catalog_schema_synthesizes_and_validates_a_dumb_solid() {
    let baseline = FcstdCodec
        .decode(&mut Cursor::new(GEOMETRY), &DecodeOptions::default())
        .unwrap();
    for target in ["fcstd:schema-2", "fcstd:schema-3", "fcstd:schema-4"] {
        let output = synthesized(GEOMETRY, target);
        assert_eq!(
            output.ir().model.bodies.len(),
            baseline.ir().model.bodies.len(),
            "{target}"
        );
        assert_eq!(
            output
                .ir()
                .source
                .as_ref()
                .unwrap()
                .dialect()
                .unwrap()
                .dialect()
                .as_str(),
            target
        );
    }
}

#[test]
fn effective_appearance_preserves_independent_body_style() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .unwrap();
    let fallback = cadmpeg_ir::topology::Color::new(0.13, 0.27, 0.61, 0.39).unwrap();
    let effective = cadmpeg_ir::topology::Color::from_rgba8(48, 96, 144, 255);
    let mut ir = decoded.ir().clone();
    ir.model.bodies[0].color = Some(fallback);
    ir.model.bodies[0].visible = Some(false);
    let mut value = serde_json::to_value(&ir).unwrap();
    let body = value["model"]["bodies"][0]["id"].clone();
    value["model"]["appearances"].as_array_mut().unwrap().push(serde_json::json!({"id":"test:model:appearance#bound", "name":"Authored display material", "base_color": effective}));
    value["model"]["appearance_bindings"].as_array_mut().unwrap().push(serde_json::json!({"id":"test:model:appearance-binding#bound", "target":{"kind":"body", "id":body}, "appearance":"test:model:appearance#bound"}));
    let ir = cadmpeg_ir::CadIr::from_json(&value.to_string()).unwrap();
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let report = plan.report().clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output
        .ir()
        .model
        .bodies
        .iter()
        .any(|body| body.color == Some(fallback) && body.visible == Some(false)));
    assert!(output
        .ir()
        .model
        .appearances
        .iter()
        .any(|appearance| appearance.base_color == Some(effective)));
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "appearance.not-transferred"));
    cadmpeg_test_support::contract::assert_written_arenas_agree(&report, &ir, output.ir());
}

#[test]
fn deleting_neutral_product_and_gui_state_does_not_replay_native_records() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    ir.model.product_definitions.clear();
    ir.model.occurrences.clear();
    ir.model.assembly_joints.clear();
    ir.model.view_presentations.clear();
    ir.model.presentation_documents.clear();
    ir.model.appearances.clear();
    ir.model.appearance_bindings.clear();
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output.ir().model.product_definitions.is_empty());
    assert!(output.ir().model.occurrences.is_empty());
    assert!(output.ir().model.assembly_joints.is_empty());
    assert!(output
        .ir()
        .model
        .presentation_documents
        .iter()
        .all(|document| document.states().is_empty()));
    assert_eq!(output.ir().model.bodies.len(), ir.model.bodies.len());
}

#[test]
fn standalone_authored_product_preserves_absent_label_and_has_no_invented_occurrence() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.product_definitions.push(serde_json::from_value(serde_json::json!({"id":"authored:model:product_definition#Prototype","kind":"part","native_ref":"fcstd:native:object#Prototype"})).unwrap());
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let report = plan.report().clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.product_definitions.len(), 1);
    assert_eq!(output.ir().model.product_definitions[0].source_name, None);
    assert_eq!(output.ir().model.product_definitions[0].label, None);
    assert!(output.ir().model.occurrences.is_empty());
    cadmpeg_test_support::contract::assert_written_arenas_agree(&report, &ir, output.ir());
}

#[test]
fn solved_profile_directions_and_parameter_attribution_survive_synthesis() {
    synthesized(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../corpus/freecad_fcstd/fixtures/design_history.FCStd"
        )),
        "fcstd:schema-4",
    );
}

#[test]
fn omitted_unreadable_feature_cannot_leave_a_dangling_dependency() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="2" Dependencies="1"><ObjectDeps Name="Bad" Count="0"/><ObjectDeps Name="Pocket" Count="1"><Dep Name="Bad"/></ObjectDeps><Object type="Part::Box" name="Bad"/><Object type="PartDesign::Pocket" name="Pocket"/></Objects><ObjectData Count="2"><Object name="Bad"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="NaN"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object><Object name="Pocket"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let input = crate::test_support::test_archive::archive_entries(&[(
        "Document.xml",
        document.as_bytes(),
    )]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(input), &DecodeOptions::default())
        .unwrap();
    let before_namespace = decoded.ir().native.namespace("fcstd").unwrap();
    let before_objects = before_namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    let before_bad = before_objects
        .iter()
        .find(|object| object.name() == "Bad")
        .unwrap();
    let before_properties = before_namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let length = before_properties
        .iter()
        .find(|property| property.owner == before_bad.id().as_str() && property.name == "Length")
        .unwrap();
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();
    assert!(plan.report().losses.iter().any(|loss| loss.code
        == cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted
            .note("")
            .code
        && loss.severity == cadmpeg_ir::report::Severity::Warning
        && loss.message.contains(&length.id)
        && loss.message.contains(&length.owner)));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let objects = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    let bad = objects
        .iter()
        .find(|object| object.name() == "Bad")
        .unwrap();
    assert_eq!(bad.type_name, "Part::Box");
    let pocket = objects
        .iter()
        .find(|object| object.name() == "Pocket")
        .unwrap();
    assert_eq!(pocket.type_name, "PartDesign::Pocket");
    assert_eq!(pocket.dependencies, [bad.id().as_str().to_owned()]);
    let properties = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    assert!(!properties
        .iter()
        .any(|property| { property.owner == bad.id().as_str() && property.name == "Length" }));
    for name in ["Width", "Height"] {
        let property = properties
            .iter()
            .find(|property| property.owner == bad.id().as_str() && property.name == name)
            .unwrap();
        assert_eq!(property.type_name, "App::PropertyLength");
        assert_eq!(
            property.values()[0].attributes["value"]
                .parse::<f64>()
                .unwrap(),
            1.0
        );
    }
    assert!(objects
        .iter()
        .all(|object| object.dependencies.iter().all(|dependency| objects
            .iter()
            .any(|target| target.id().as_str() == dependency))));
}

#[test]
fn independent_linked_prototype_placement_is_baked_and_retains_its_decomposition() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    let occurrence = ir
        .model
        .occurrences
        .iter_mut()
        .find(|occurrence| occurrence.linked_prototype.is_some())
        .unwrap();
    let name = occurrence.name.clone();
    occurrence.linked_prototype = Some(
        cadmpeg_ir::transform::Transform::affine([
            [1.0, 0.0, 0.0, 17.0],
            [0.0, 1.0, 0.0, 23.0],
            [0.0, 0.0, 1.0, 31.0],
        ])
        .unwrap(),
    );
    let expected = occurrence.clone();
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let report = plan.report().clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let actual = output
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name == name)
        .unwrap();
    assert_eq!(actual.transform, expected.transform);
    assert_eq!(actual.linked_prototype, expected.linked_prototype);
    let native = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::ProductNodeRecord>("product_nodes")
        .unwrap();
    let native = native
        .iter()
        .find(|record| Some(record.object.as_str()) == actual.native_ref.as_deref())
        .unwrap();
    assert_eq!(native.link_transform(), Some(false));
    assert_eq!(
        native.local_transform().unwrap().transform(),
        expected
            .effective_transform()
            .unwrap()
            .expect("authored placement")
    );
    cadmpeg_test_support::contract::assert_written_arenas_agree(&report, &ir, output.ir());
}

#[test]
fn exact_editor_feature_expression_constraint_and_appearance_edits_survive() {
    use serde_json::json;
    for (source_index, source) in [
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../corpus/freecad_fcstd/fixtures/design_history.FCStd"
        ))
        .as_slice(),
        CORE_DESIGN_PRODUCT,
    ]
    .into_iter()
    .enumerate()
    {
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        if source_index == 0 {
            assert_eq!(decoded.ir().model.parameters.len(), 16);
        }
        let mut value = serde_json::to_value(decoded.ir()).unwrap();
        let features = value["model"]["features"].as_array_mut().unwrap();
        features
            .iter_mut()
            .find(|feature| {
                feature["id"]
                    .as_str()
                    .is_some_and(|id| id.ends_with("#Pad") || id.ends_with("#DesignedFeature"))
            })
            .unwrap()["name"] = json!("AuthoredPad");
        let parameters = value["model"]["parameters"].as_array_mut().unwrap();
        parameters
            .iter_mut()
            .find(|parameter| {
                parameter["id"].as_str().is_some_and(|id| {
                    id.ends_with("#Pad:Length") || id.ends_with("#DesignedFeature:Length")
                })
            })
            .unwrap()["expression"] = json!(cadmpeg_ir::features::Expression::text("37 mm"));
        parameters
            .iter_mut()
            .find(|parameter| {
                parameter["id"].as_str().is_some_and(|id| {
                    id.ends_with("#Pad:Length") || id.ends_with("#DesignedFeature:Length")
                })
            })
            .unwrap()["value"] = json!({"kind":"length","value":37.0});
        let dimensional = parameters
            .iter_mut()
            .find(|parameter| {
                parameter["id"]
                    .as_str()
                    .is_some_and(|id| id.contains(":constraint:"))
            })
            .unwrap();
        dimensional["expression"] = json!(cadmpeg_ir::features::Expression::text("31 mm"));
        dimensional["value"] = json!({"kind":"length","value":31.0});
        let body = value["model"]["bodies"][0]["id"].clone();
        let color = cadmpeg_ir::topology::Color::from_rgba8(48, 96, 144, 255);
        if let Some(appearance) = value["model"]["appearances"]
            .as_array_mut()
            .unwrap()
            .first_mut()
        {
            appearance["base_color"] = json!(color);
        } else {
            value["model"]["appearances"].as_array_mut().unwrap().push(json!({"id":"cadir:model:appearance#authored","name":"Authored appearance","base_color":color}));
            value["model"]["appearance_bindings"].as_array_mut().unwrap().push(json!({"id":"cadir:model:appearance-binding#authored","target":{"kind":"body","id":body},"appearance":"cadir:model:appearance#authored"}));
        }
        let mut edited = cadmpeg_ir::CadIr::from_json(&value.to_string()).unwrap();
        let changed = edited
            .model
            .parameters
            .iter()
            .filter(|parameter| {
                matches!(
                    parameter.expression.literal_text().as_deref(),
                    Some("37 mm" | "31 mm")
                )
            })
            .map(|parameter| {
                (
                    parameter.id.clone(),
                    parameter.expression.clone(),
                    parameter.value.clone(),
                )
            })
            .collect::<Vec<_>>();
        for (id, expression, value) in changed {
            edited
                .set_parameter_expression(&id, expression, value)
                .unwrap();
        }
        let plan = FcstdCodec
            .plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)
            .unwrap();
        let report = plan.report().clone();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let output = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        if source_index == 0 {
            assert_eq!(output.ir().model.parameters.len(), 16);
            assert!(!output
                .ir()
                .model
                .parameters
                .iter()
                .any(|parameter| parameter.name == "FuzzyTolerance"));
        }
        for feature in &edited.model.features {
            if let cadmpeg_ir::features::FeatureDefinition::PostProcess {
                fuzzy_tolerance, ..
            } = feature.definition()
            {
                let written = output
                    .ir()
                    .model
                    .features
                    .iter()
                    .find(|written| written.id == feature.id)
                    .unwrap();
                assert!(
                    matches!(written.definition(), cadmpeg_ir::features::FeatureDefinition::PostProcess { fuzzy_tolerance: actual, .. } if actual == fuzzy_tolerance)
                );
            }
        }
        assert!(
            output.source_fidelity().unrepresented().is_empty(),
            "{:?}",
            output.report().losses
        );
        assert!(
            output.report().losses.is_empty(),
            "{:?}",
            output.report().losses
        );
        assert!(output
            .ir()
            .model
            .features
            .iter()
            .any(|feature| feature.name.as_deref() == Some("AuthoredPad")));
        assert!(output
            .ir()
            .model
            .parameters
            .iter()
            .any(|parameter| parameter.expression.literal_text().as_deref() == Some("37 mm")));
        assert!(output.ir().model.parameters.iter().any(|parameter|parameter.expression.literal_text().as_deref()==Some("31 mm") && matches!(&parameter.value,Some(cadmpeg_ir::features::ParameterValue::Length(value)) if value.get()==31.0)));
        assert!(output
            .ir()
            .model
            .appearance_bindings
            .iter()
            .any(|binding| output
                .ir()
                .model
                .appearances
                .iter()
                .any(|appearance| appearance.id == binding.appearance
                    && appearance.base_color == Some(color))));
        for arena in [
            "features",
            "sketches",
            "sketch_entities",
            "sketch_constraints",
            "parameters",
            "bodies",
            "spreadsheets",
            "product_definitions",
            "occurrences",
        ] {
            assert_eq!(
                serde_json::to_value(&edited.model).unwrap()[arena]
                    .as_array()
                    .unwrap()
                    .len(),
                serde_json::to_value(&output.ir().model).unwrap()[arena]
                    .as_array()
                    .unwrap()
                    .len(),
                "{arena}"
            );
        }
        cadmpeg_test_support::contract::assert_written_arenas_agree(&report, &edited, output.ir());
    }
}

#[test]
fn product_metadata_does_not_replace_a_typed_vendor_construction_object() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="2"><Object name="Root" type="App::Part"/><Object name="Vendor" type="Vendor::PartDesign::PadLike"/></Objects><ObjectData Count="2"><Object name="Root"><Properties Count="1"><Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Vendor"/></LinkList></Property></Properties></Object><Object name="Vendor"><Properties Count="1"><Property name="Length" type="App::PropertyLength"><Float value="5"/></Property></Properties></Object></ObjectData></Document>"#;
    let input = crate::test_support::test_archive::archive_entries(&[(
        "Document.xml",
        document.as_bytes(),
    )]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(input), &DecodeOptions::default())
        .unwrap();
    assert!(decoded.source_fidelity().unrepresented().is_empty());
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let report = plan.report().clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let objects = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    assert!(objects
        .iter()
        .any(|object| object.type_name == "Vendor::PartDesign::PadLike"));
    let properties = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    assert!(properties
        .iter()
        .all(|property| property.name != "ScaleVector"));
    assert!(output.ir().model.features.is_empty());
    assert_eq!(output.ir().model.product_definitions.len(), 2);
    cadmpeg_test_support::contract::assert_written_arenas_agree(&report, decoded.ir(), output.ir());
}
