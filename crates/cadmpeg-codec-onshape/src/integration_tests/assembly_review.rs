// SPDX-License-Identifier: Apache-2.0
//! Assembly placement, reference, and bounded-work regressions.
use crate::test_support::{assembly::*, read_bytes, Report};
use cadmpeg_core::json;
use cadmpeg_ir::products::{Occurrence, PlaneSense};
use serde_json::Value;
use std::path::Path;

fn decode(bytes: &[u8]) -> (crate::reader::Model, Report) {
    let mut report = Report::default();
    let model = read_bytes(Path::new("synthetic.onshape-bundle"), bytes, &mut report).unwrap();
    let validation = cadmpeg_ir::validate::validate_neutral(&model.ir, Vec::new()).unwrap();
    assert!(
        !validation
            .findings
            .iter()
            .any(|finding| finding.severity == cadmpeg_ir::report::Severity::Error),
        "{:?}",
        validation.findings
    );
    (model, report)
}
fn read(definition: Value) -> (crate::reader::Model, Report) {
    decode(&bundle(definition))
}
fn occurrence<'a>(model: &'a crate::reader::Model, name: &str) -> &'a Occurrence {
    model
        .ir
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some(name))
        .unwrap()
}
fn loss(report: &Report, code: &str) -> bool {
    report
        .losses()
        .iter()
        .any(|loss| loss.code.as_deref() == Some(code))
}
#[test]
fn rotated_parent_produces_child_local_translation_and_rotation() {
    let (model, _) = read(nested());
    assert_eq!(
        occurrence(&model, "first").transform.unwrap().affine_rows(),
        [[1., 0., 0., 10.], [0., 1., 0., 0.], [0., 0., 1., 0.]]
    );
    assert_eq!(
        occurrence(&model, "second")
            .transform
            .unwrap()
            .affine_rows(),
        [[1., 0., 0., 20.], [0., 1., 0., 0.], [0., 0., 1., 0.]]
    );
}
#[test]
fn unresolved_subassembly_world_transform_leaves_children_unplaced() {
    for malformed in [false, true] {
        let mut definition = nested();
        if malformed {
            definition["rootAssembly"]["occurrences"][0]["transform"] = json!([1., 2.]);
        } else {
            definition["rootAssembly"]["occurrences"][0]
                .as_object_mut()
                .unwrap()
                .remove("transform");
        }
        let (model, report) = read(definition);
        assert!(occurrence(&model, "sub").transform.is_none());
        assert!(occurrence(&model, "first").transform.is_none());
        assert!(occurrence(&model, "second").transform.is_none());
        assert!(loss(&report, "onshape/assembly.record-unsupported"));
        assert!(model
            .ir
            .model
            .assembly_joints
            .iter()
            .all(|joint| joint.frame_sense().is_none()));
    }
}
#[test]
fn nested_mates_and_root_mate_paths_resolve_the_same_children() {
    let mut definition = nested();
    let mut root_mate = mate("rootMate", "FASTENED");
    for entity in root_mate["featureData"]["matedEntities"]
        .as_array_mut()
        .unwrap()
    {
        entity["matedOccurrence"]
            .as_array_mut()
            .unwrap()
            .insert(0, json!("sub"));
    }
    definition["rootAssembly"]["features"] = json!([root_mate]);
    let (model, report) = read(definition);
    assert_eq!(model.ir.model.assembly_joints.len(), 2);
    let ids: Vec<_> = model
        .ir
        .model
        .assembly_joints
        .iter()
        .map(|joint| {
            joint
                .connectors()
                .map(|connector| connector.operand.clone())
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(ids[0], ids[1]);
    assert!(loss(&report, "onshape/assembly.mate-frame-assumed"));
}
#[test]
fn second_rotated_connector_keeps_millimetres_and_solved_opposed_sense() {
    for kind in [
        "PLANAR",
        "REVOLUTE",
        "FASTENED",
        "SLIDER",
        "CYLINDRICAL",
        "BALL",
    ] {
        let mut definition = definition();
        let mut feature = mate("opposed", kind);
        feature["featureData"]["matedEntities"][1]["matedCS"] = json!({"origin":[0.004,0.005,0.006],"xAxis":[-1.,0.,0.],"yAxis":[0.,1.,0.],"zAxis":[0.,0.,-1.]});
        definition["rootAssembly"]["features"] = json!([feature]);
        let (model, report) = read(definition);
        let joint = model
            .ir
            .model
            .assembly_joints
            .iter()
            .find(|joint| joint.connectors().count() == 2)
            .unwrap();
        assert_eq!(joint.frame_sense(), Some(PlaneSense::Opposed));
        assert_eq!(
            joint.connectors().nth(1).unwrap().frame.affine_rows(),
            [[-1., 0., 0., 4.], [0., 1., 0., 5.], [0., 0., -1., 6.]]
        );
        assert!(!loss(&report, "onshape/assembly.mate-limits-unresolved"));
    }
}
#[test]
fn planar_without_solved_world_sense_keeps_its_native_response() {
    let mut definition = definition();
    definition["rootAssembly"]["occurrences"][1]
        .as_object_mut()
        .unwrap()
        .remove("transform");
    definition["rootAssembly"]["features"] = json!([mate("planar", "PLANAR")]);
    let (model, report) = read(definition);
    assert_eq!(model.ir.model.assembly_joints.len(), 1);
    assert!(loss(&report, "onshape/assembly.mate-frame-unresolved"));
}
#[test]
fn hidden_and_inherited_suppression_reach_children_and_mates() {
    let mut definition = nested();
    definition["rootAssembly"]["instances"][0]["suppressed"] = json!(true);
    definition["rootAssembly"]["occurrences"][1]["hidden"] = json!(true);
    definition["rootAssembly"]["occurrences"][2]["hidden"] = json!(false);
    let (model, _) = read(definition);
    for name in ["sub", "first", "second"] {
        assert_eq!(occurrence(&model, name).suppressed, Some(true));
    }
    assert_eq!(occurrence(&model, "first").visible, Some(false));
    assert_eq!(occurrence(&model, "second").visible, Some(true));
    assert!(model
        .ir
        .model
        .assembly_joints
        .iter()
        .all(|joint| joint.suppressed));
}
#[test]
fn missing_definition_or_root_and_duplicate_instances_remain_partial() {
    let missing = edited(definition(), |members| {
        members
            .as_object_mut()
            .unwrap()
            .remove("assemblies/assembly/definition.json");
    });
    let (model, report) = decode(&missing);
    assert_eq!(model.ir.geometry.bodies.len(), 1);
    assert!(loss(&report, "onshape/assembly.record-unsupported"));
    let (model, report) = read(json!({"parts":[],"subAssemblies":[]}));
    assert!(model.ir.model.occurrences.is_empty());
    assert!(loss(&report, "onshape/assembly.record-unsupported"));
    let mut definition = definition();
    let first = definition["rootAssembly"]["instances"][0].clone();
    definition["rootAssembly"]["instances"]
        .as_array_mut()
        .unwrap()
        .push(first);
    let (model, report) = read(definition);
    assert_eq!(model.ir.model.occurrences.len(), 2);
    assert!(loss(&report, "onshape/assembly.record-repeated"));
}
#[test]
fn repeated_subassembly_has_distinct_paths_and_local_joints() {
    let mut definition = nested();
    let mut repeated = definition["rootAssembly"]["instances"][0].clone();
    repeated["id"] = json!("otherSub");
    repeated["name"] = json!("otherSub");
    definition["rootAssembly"]["instances"]
        .as_array_mut()
        .unwrap()
        .push(repeated);
    let occurrences = definition["rootAssembly"]["occurrences"]
        .as_array()
        .unwrap()
        .clone();
    for mut occurrence in occurrences {
        occurrence["path"][0] = json!("otherSub");
        definition["rootAssembly"]["occurrences"]
            .as_array_mut()
            .unwrap()
            .push(occurrence);
    }
    let (model, _) = read(definition);
    assert_eq!(model.ir.model.occurrences.len(), 6);
    assert_eq!(model.ir.model.assembly_joints.len(), 2);
    assert_ne!(
        model.ir.model.assembly_joints[0].id,
        model.ir.model.assembly_joints[1].id
    );
    assert_ne!(
        model.ir.model.assembly_joints[0]
            .connectors()
            .next()
            .unwrap()
            .operand,
        model.ir.model.assembly_joints[1]
            .connectors()
            .next()
            .unwrap()
            .operand
    );
}
#[test]
fn non_default_configuration_and_microversion_mismatch_remain_unbound() {
    for (field, value) in [
        ("configuration", "alternate"),
        ("documentMicroversion", "other-microversion"),
    ] {
        let mut definition = definition();
        definition["rootAssembly"]["instances"][0][field] = json!(value);
        let (model, report) = read(definition);
        let prototype = &occurrence(&model, "first").prototype;
        assert!(model
            .ir
            .model
            .product_definitions
            .iter()
            .find(|part| &part.id == prototype)
            .unwrap()
            .bodies
            .is_empty());
        assert!(loss(&report, "onshape/assembly.reference-unresolved"));
        assert!(report
            .losses()
            .iter()
            .any(|loss| loss.detail.contains(value)));
    }
}
#[test]
fn external_part_identity_and_manifest_microversion_remain_in_loss_detail() {
    let mut definition = definition();
    for (ordinal, instance) in definition["rootAssembly"]["instances"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        instance["documentId"] = json!("external");
        instance["partId"] = json!(format!("external-part-{ordinal}"));
        instance["configuration"] = json!("alternate");
    }
    let bytes = edited(
        definition,
        |members| members["manifest.json"]["external"] = json!([{"documentId":"external","elementId":"studio","microversionId":"external-microversion"}]),
    );
    let (model, report) = decode(&bytes);
    assert_ne!(
        occurrence(&model, "first").prototype,
        occurrence(&model, "second").prototype
    );
    for part in ["external-part-0", "external-part-1"] {
        assert!(report.losses().iter().any(|loss| loss.detail.contains(part)
            && loss.detail.contains("external-microversion")
            && loss.detail.contains("alternate")));
    }
}
#[test]
fn assembly_root_forms_and_element_characters_are_explicit() {
    for root in [json!("assembly"), json!({"id":"assembly"}), Value::Null] {
        let bytes = edited(definition(), |members| {
            members["manifest.json"]["root_element"] = root;
        });
        assert_eq!(decode(&bytes).0.ir.model.occurrences.len(), 2);
    }
    let bytes = edited(definition(), |members| {
        members["manifest.json"]
            .as_object_mut()
            .unwrap()
            .remove("root_element");
    });
    assert_eq!(decode(&bytes).0.ir.model.occurrences.len(), 2);
    for root in [json!("../assembly"), json!(7), json!({"id":true})] {
        let bytes = edited(definition(), |members| {
            members["manifest.json"]["root_element"] = root;
        });
        let (model, report) = decode(&bytes);
        assert!(model.ir.model.occurrences.is_empty());
        assert!(loss(&report, "onshape/assembly.record-unsupported"));
    }
}
#[test]
fn non_mate_features_have_one_specific_loss_and_plain_mates_have_no_limit_loss() {
    let mut definition = definition();
    definition["rootAssembly"]["features"] = json!([
        {"id":"connector","featureType":"mateConnector"},
        {"id":"gear","featureType":"gearRelation"},
        {"id":"rack","featureType":"rackRelation"},
        {"id":"screw","featureType":"screwRelation"},
        mate("plain","REVOLUTE")
    ]);
    let (model, report) = read(definition);
    assert_eq!(model.ir.model.assembly_joints.len(), 2);
    assert_eq!(
        report
            .losses()
            .iter()
            .filter(|loss| loss.code.as_deref() == Some("onshape/assembly.feature-unsupported"))
            .count(),
        4
    );
    assert!(!loss(&report, "onshape/assembly.mate-limits-unresolved"));
    assert!(!report
        .losses()
        .iter()
        .any(|loss| loss.detail.contains("mateType is missing")));
}
#[test]
fn mate_quantity_losses_name_enabled_and_unknown_parameter_ids() {
    for (parameters, expected) in [
        (
            json!([{"parameterId":"mateType","value":"REVOLUTE"},{"parameterId":"limitsEnabled","value":false},{"parameterId":"limitMinimum","value":-1.}]),
            None,
        ),
        (json!({"mateType":"REVOLUTE"}), None),
        (json!([{"parameterId":"offsetX","expression":"0 mm"}]), None),
        (
            json!([{"parameterId":"limitMinimum","value":0.0}]),
            Some("limitMinimum"),
        ),
        (json!([{"parameterId":"limitsEnabled","value":true}]), None),
        (json!([{"parameterId":"offsetEnabled","value":true}]), None),
        (
            json!([{"parameterId":"unknownControl","value":false}]),
            Some("unknownControl"),
        ),
    ] {
        let mut definition = definition();
        definition["rootAssembly"]["features"] = json!([mate("nativeMate", "REVOLUTE")]);
        let bytes = edited(definition, |members| {
            members["assemblies/assembly/features.json"]["features"] =
                json!([{"featureId":"nativeMate","parameters":parameters}]);
        });
        let (_, report) = decode(&bytes);
        assert_eq!(
            loss(&report, "onshape/assembly.mate-limits-unresolved"),
            expected.is_some()
        );
        if let Some(id) = expected {
            assert!(report.losses().iter().any(|loss| loss.detail.contains(id)));
        }
    }
}

#[test]
fn depth_limit_preserves_the_reference_at_the_last_supported_level() {
    let assembly = |ordinal: usize| json!({"id":format!("sub-{ordinal}"),"name":format!("sub-{ordinal}"),"type":"Assembly","documentId":"document","elementId":format!("nested-{ordinal}"),"configuration":"default"});
    let mut definition = definition();
    definition["rootAssembly"]["instances"] = json!([assembly(0)]);
    definition["rootAssembly"]["occurrences"] = json!([]);
    definition["subAssemblies"] = json!([]);
    let mut path = Vec::new();
    for ordinal in 0..66 {
        path.push(format!("sub-{ordinal}"));
        definition["rootAssembly"]["occurrences"]
            .as_array_mut()
            .unwrap()
            .push(json!({"path":path,"transform":placement(0.,0.)}));
        definition["subAssemblies"].as_array_mut().unwrap().push(json!({"documentId":"document","elementId":format!("nested-{ordinal}"),"configuration":"default","instances":[assembly(ordinal+1)]}));
    }
    let (model, report) = read(definition);
    assert_eq!(model.ir.model.occurrences.len(), 64);
    assert!(loss(&report, "onshape/assembly.reference-unresolved"));
    assert_eq!(occurrence(&model, "sub-63").name.as_deref(), Some("sub-63"));
}
#[test]
fn repeated_bad_mates_have_bounded_work_and_aggregated_diagnostics() {
    let mut definition = nested();
    definition["rootAssembly"]["instances"] = json!([]);
    definition["rootAssembly"]["occurrences"] = json!([]);
    for ordinal in 0..40 {
        let sub = format!("sub-{ordinal}");
        definition["rootAssembly"]["instances"].as_array_mut().unwrap().push(json!({"id":sub,"name":sub,"type":"Assembly","documentId":"document","elementId":"nested","configuration":"default"}));
        for path in [
            vec![sub.clone()],
            vec![sub.clone(), "first".into()],
            vec![sub, "second".into()],
        ] {
            definition["rootAssembly"]["occurrences"]
                .as_array_mut()
                .unwrap()
                .push(json!({"path":path,"transform":placement(0.,0.)}));
        }
    }
    definition["subAssemblies"][0]["features"] = json!((0..500)
        .map(|ordinal| json!({"id":format!("gear-{ordinal}"),"featureType":"gearRelation"}))
        .collect::<Vec<_>>());
    let (model, report) = read(definition);
    assert_eq!(model.ir.model.occurrences.len(), 120);
    assert!(report.losses().len() <= 260);
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.detail.contains("diagnostics aggregated")));
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.detail.contains("work exceeds the row limit")));
}

fn names_from_step<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'a>,
    view: cadmpeg_core::decode::View<'a>,
) -> Result<cadmpeg_ir::codec::Decoded, cadmpeg_core::CodecError> {
    use cadmpeg_ir::codec::CodecBackend;
    let mut decoded = cadmpeg_codec_step::StepCodec::default().decode_impl(ctx, view)?;
    for (body, name) in decoded.ir.geometry.bodies.iter_mut().zip(["left", "right"]) {
        body.name = Some(name.into());
    }
    Ok(decoded)
}
fn unnamed_step<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'a>,
    view: cadmpeg_core::decode::View<'a>,
) -> Result<cadmpeg_ir::codec::Decoded, cadmpeg_core::CodecError> {
    use cadmpeg_ir::codec::CodecBackend;
    let mut decoded = cadmpeg_codec_step::StepCodec::default().decode_impl(ctx, view)?;
    for body in &mut decoded.ir.geometry.bodies {
        body.name = None;
    }
    Ok(decoded)
}
#[test]
fn unique_part_names_bind_two_bodies_and_duplicate_names_stay_unbound() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;
    for duplicate in [false, true] {
        let mut definition = definition();
        definition["rootAssembly"]["instances"][0]["partId"] = json!("left-part");
        definition["rootAssembly"]["instances"][1]["partId"] = json!("right-part");
        definition["parts"] = json!([
            {"documentId":"document","elementId":"studio","configuration":"default","partId":"left-part","name":"left"},
            {"documentId":"document","elementId":"studio","configuration":"default","partId":"right-part","name":if duplicate {"left"} else {"right"}}
        ]);
        let bytes = crate::test_support::review::members(&bundle(definition), |members| {
            members.insert(
                "partstudios/studio/export.step".into(),
                crate::test_support::review::two_body_step(),
            );
        });
        let result = crate::OnshapeCodec::new(names_from_step)
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let (ir, report, _) = result.into_parts();
        for occurrence in &ir.model.occurrences {
            let prototype = ir
                .model
                .product_definitions
                .iter()
                .find(|product| product.id == occurrence.prototype)
                .unwrap();
            assert_eq!(prototype.bodies.len(), usize::from(!duplicate));
            if !duplicate {
                let body = ir
                    .geometry
                    .bodies
                    .iter()
                    .find(|body| prototype.bodies.contains(&body.id))
                    .unwrap();
                assert_eq!(body.name, prototype.label);
            }
        }
        assert_eq!(
            report
                .losses
                .iter()
                .any(|loss| loss.code.to_string() == "onshape/assembly.reference-unresolved"),
            duplicate
        );
    }
}
#[test]
fn single_part_and_single_unnamed_body_bind_without_a_name_claim() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;
    let result = crate::OnshapeCodec::new(unnamed_step)
        .decode(
            &mut Cursor::new(bundle(definition())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let (ir, report, _) = result.into_parts();
    assert!(ir.geometry.bodies[0].name.is_none());
    for occurrence in &ir.model.occurrences {
        let prototype = ir
            .model
            .product_definitions
            .iter()
            .find(|product| product.id == occurrence.prototype)
            .unwrap();
        assert_eq!(prototype.bodies, vec![ir.geometry.bodies[0].id.clone()]);
    }
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.code.to_string() == "onshape/assembly.reference-unresolved"));
}

#[test]
fn unresolved_intermediate_placement_does_not_use_a_rotated_grandparent() {
    for malformed in [false, true] {
        let mut definition = nested();
        let child_assembly = definition["rootAssembly"]["instances"][0].clone();
        definition["rootAssembly"]["instances"][0]["id"] = json!("outer");
        definition["rootAssembly"]["instances"][0]["name"] = json!("outer");
        definition["rootAssembly"]["instances"][0]["elementId"] = json!("outer-definition");
        definition["rootAssembly"]["occurrences"][0]["path"] = json!(["outer"]);
        for ordinal in [1, 2] {
            definition["rootAssembly"]["occurrences"][ordinal]["path"]
                .as_array_mut()
                .unwrap()
                .insert(0, json!("outer"));
        }
        let mut unresolved = json!({"path":["outer","sub"]});
        if malformed {
            unresolved["transform"] = json!([1., 2.]);
        }
        definition["rootAssembly"]["occurrences"]
            .as_array_mut()
            .unwrap()
            .push(unresolved);
        definition["subAssemblies"].as_array_mut().unwrap().push(json!({"documentId":"document","elementId":"outer-definition","configuration":"default","instances":[child_assembly]}));
        let (model, report) = read(definition);
        assert_eq!(
            occurrence(&model, "outer").transform.unwrap().affine_rows(),
            [[0., -1., 0., 10.], [1., 0., 0., 20.], [0., 0., 1., 0.]]
        );
        for name in ["sub", "first", "second"] {
            assert!(occurrence(&model, name).transform.is_none());
        }
        assert!(loss(&report, "onshape/assembly.record-unsupported"));
    }
}
#[test]
fn solved_mate_sense_uses_the_occurrence_rotation() {
    let mut definition = definition();
    definition["rootAssembly"]["occurrences"][1]["transform"] =
        json!([1., 0., 0., 0., 0., -1., 0., 0., 0., 0., -1., 0., 0., 0., 0., 1.]);
    definition["rootAssembly"]["features"] = json!([mate("world-opposed", "PLANAR")]);
    let (model, _) = read(definition);
    let joint = model
        .ir
        .model
        .assembly_joints
        .iter()
        .find(|joint| joint.connectors().count() == 2)
        .unwrap();
    assert_eq!(joint.frame_sense(), Some(PlaneSense::Opposed));
    assert_eq!(
        joint.connectors().next().unwrap().frame,
        joint.connectors().nth(1).unwrap().frame
    );
}
