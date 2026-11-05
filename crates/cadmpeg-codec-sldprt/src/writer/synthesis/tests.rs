// SPDX-License-Identifier: Apache-2.0
//! Export planning owns patch fallback and per-body degradation.

use crate::SldprtCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::{ArenaClass, CadIr};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::report::export::WritePath;
use std::io::Cursor;

#[test]
fn cascade_names_each_feature_with_a_removed_regeneration_parent() {
    use cadmpeg_ir::features::{
        DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
        FeatureOperation,
    };

    let mut ir = crate::test_support::ir::source_less_cube();
    let parent = FeatureId::mint("synthetic:test:feature#parent").unwrap();
    let child = FeatureId::mint("synthetic:test:feature#child").unwrap();
    let missing = FeatureId::mint("synthetic:test:feature#missing").unwrap();
    for (ordinal, id) in [parent.clone(), child.clone()].into_iter().enumerate() {
        ir.model.features.push(Feature {
            product_definition: None,
            id,
            ordinal: cadmpeg_core::decode::u64_from_index(ordinal),
            name: None,
            suppressed: Some(false),
            needs_recompute: false,
            dependencies: DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::default(),
            source_tag: None,
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::StoredGeometry {},
            )),
            native_ref: None,
        });
    }
    ir.model
        .set_feature_regeneration_parent(&child, &parent)
        .unwrap();
    ir.model.features[0].dependencies = DistinctMembers::try_from(vec![missing]).unwrap();
    let mut losses = Vec::new();
    super::cascade_validation_errors(&mut ir, &mut losses).unwrap();
    for id in [parent, child] {
        assert!(!ir.model.features.iter().any(|feature| feature.id == id));
        assert!(losses.iter().any(|loss| loss.message.contains(id.as_str())));
    }
}

#[test]
fn design_only_document_keeps_its_scope() {
    let source = crate::test_support::carchive::sldprt_with_archive_parameter();
    let input = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(input.ir().scope.design);
    assert!(!input.ir().scope.model);
    let plan = SldprtCodec
        .plan(EncodeInput::new(input.ir(), None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().scope, input.ir().scope);
    let before = crate::test_support::native::sldprt_native(input.ir());
    let after = crate::test_support::native::sldprt_native(output.ir());
    for attribute in before
        .document_metadata
        .iter()
        .flat_map(|record| &record.attributes)
    {
        if crate::writer::MetadataRecord::project(attribute, 0.001, |_| Ok(()))
            .unwrap()
            .is_some()
        {
            assert!(after
                .document_metadata
                .iter()
                .flat_map(|record| &record.attributes)
                .any(|actual| actual.name == attribute.name && actual.values == attribute.values));
        }
    }
}

#[test]
fn design_only_configuration_does_not_create_a_model_partition() {
    use cadmpeg_ir::features::{ConfigurationId, DesignConfiguration};
    let source = crate::test_support::carchive::sldprt_with_archive_parameter();
    let input = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = input.ir().clone();
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("synthetic:test:configuration#design-only").unwrap(),
        ordinal: 0,
        active: true,
        source_index: Some(0),
        name: Some("Default".into()),
        material: None,
        properties: std::collections::BTreeMap::default(),
        bodies: None,
        parameter_values: std::collections::BTreeMap::default(),
        parameter_overrides: std::collections::BTreeMap::default(),
        feature_states: std::collections::BTreeMap::default(),
        native_ref: None,
    });
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().scope, ir.scope);
    assert_eq!(output.ir().model.configurations.len(), 1);
}

#[test]
fn empty_authored_document_produces_a_native_file_and_empty_model_loss() {
    let ir = CadIr::empty();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("an empty model is a valid native document");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "export.no-exportable-solids"));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write planned empty part");
    SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("empty native part parses");
}

#[test]
fn unrepresented_source_content_never_reaches_patch_or_replay() {
    let source = crate::test_support::history::sldprt_with_body_and_history(
        &crate::test_support::parasolid::triangle_body(),
    );
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("fixture decodes");
    let (ir, _, mut fidelity) = decoded.into_parts();
    let retained = fidelity
        .retained_records()
        .keys()
        .next()
        .expect("retained source record")
        .to_string();
    fidelity
        .mark_untyped(&retained, [ArenaClass::Design])
        .expect("mark retained source");
    // D/E1: unrepresented source is rejected before either retained layout path.
    for edited in [false, true] {
        let mut document = ir.clone();
        if edited {
            document.model.faces[0].name = Some("authored face".into());
        }
        let plan = SldprtCodec
            .plan(
                EncodeInput::new(&document, Some(&fidelity)),
                TargetRequest::Inherit,
            )
            .expect("source content absent from the document falls back to synthesis");
        assert!(matches!(
            plan.report().write_path(),
            WritePath::Synthesized { .. }
        ));
        assert!(
            matches!(plan.report().synthesis_cause(), Some(cadmpeg_ir::codec::write::SynthesisCause::SourceUnrepresented { classes }) if classes == &cadmpeg_ir::document::ArenaClasses::from([ArenaClass::Design]))
        );
    }
}

#[test]
fn unsupported_body_does_not_discard_an_exportable_body() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube admitted");
    let second_json = serde_json::to_string(&ir)
        .expect("serialize fixture")
        .replace("synthetic:cube:", "synthetic:second:");
    let second: CadIr = serde_json::from_str(&second_json).expect("distinct cube fixture");
    ir.model.surfaces[0].geometry =
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None });
    macro_rules! append {
        ($($arena:ident),* $(,)?) => {$(ir.model.$arena.extend(second.model.$arena);)*};
    }
    append!(
        bodies, regions, shells, faces, loops, coedges, edges, vertices, points, surfaces, curves
    );
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("the other body can be exported");
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "writer.entity-omitted"
            && loss.severity == cadmpeg_ir::report::Severity::Error));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write supported part");
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("synthesized part parses");
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 6);
}

#[test]
fn neutral_parameters_survive_source_less_synthesis() {
    let source = crate::test_support::history::sldprt_with_body_and_history(
        &crate::test_support::parasolid::triangle_body(),
    );
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("fixture decodes");
    let mut ir = decoded.ir().clone();
    ir.source = None;
    ir.native.0.clear();
    for feature in &mut ir.model.features {
        feature.native_ref = None;
    }
    for parameter in &mut ir.model.parameters {
        parameter.native_ref = None;
    }
    for configuration in &mut ir.model.configurations {
        configuration.native_ref = None;
    }
    assert!(!ir.model.parameters.is_empty());
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("neutral features and parameters generate native history");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write synthesized part");
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("synthesized history decodes");
    assert_eq!(
        decoded.ir().model.parameters.len(),
        ir.model.parameters.len()
    );
    assert_eq!(
        decoded.ir().model.parameters[0].expression,
        ir.model.parameters[0].expression
    );
}

#[test]
fn global_neutral_parameter_generates_an_equations_owner() {
    let source = crate::test_support::history::sldprt_with_body_and_history(
        &crate::test_support::parasolid::triangle_body(),
    );
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("fixture decodes");
    let mut wire = serde_json::to_value(decoded.ir()).expect("fixture serializes");
    wire.as_object_mut()
        .expect("document object")
        .remove("source");
    wire["native"] = serde_json::json!({});
    wire["model"]["features"] = serde_json::json!([]);
    wire["model"]["configurations"] = serde_json::json!([]);
    for parameter in wire["model"]["parameters"]
        .as_array_mut()
        .expect("parameter array")
    {
        let parameter = parameter.as_object_mut().expect("parameter object");
        parameter.remove("owner");
        parameter.remove("native_ref");
    }
    let ir: CadIr = serde_json::from_value(wire).expect("authored global parameter document");
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("global parameter gets a native equations owner");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write global parameter");
    let written = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("generated equations decode");
    assert_eq!(
        written.ir().model.parameters.len(),
        ir.model.parameters.len()
    );
    assert_eq!(
        written.ir().model.parameters[0].expression,
        ir.model.parameters[0].expression
    );
    assert!(written.ir().model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::TreeNode {
                role: cadmpeg_ir::features::FeatureTreeNodeRole::Equations,
                ..
            }
        )
    )));
}

#[test]
fn native_history_cannot_restore_deleted_neutral_features() {
    let source = crate::test_support::history::sldprt_with_body_and_history(
        &crate::test_support::parasolid::triangle_body(),
    );
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("fixture decodes");
    let (ir, _, fidelity) = decoded.into_parts();
    assert!(!ir.model.features.is_empty());
    let mut wire = serde_json::to_value(&ir).expect("fixture serializes");
    wire["model"]["features"] = serde_json::json!([]);
    wire["model"]["parameters"] = serde_json::json!([]);
    for configuration in wire["model"]["configurations"]
        .as_array_mut()
        .expect("configuration array")
    {
        let configuration = configuration.as_object_mut().expect("configuration object");
        configuration.remove("feature_states");
        configuration.remove("parameter_values");
        configuration.remove("parameter_overrides");
    }
    let ir: CadIr = serde_json::from_value(wire)
        .expect("deleted feature relations are absent from the document");
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(&ir, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .expect("deleted neutral design synthesizes");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write deleted design");
    let written = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("output decodes");
    assert!(written.ir().model.features.is_empty());
    assert!(written.ir().model.parameters.is_empty());
    assert_eq!(written.ir().model.bodies.len(), ir.model.bodies.len());
}

#[test]
fn native_properties_cannot_restore_deleted_neutral_attributes() {
    let mut source = crate::test_support::history::sldprt_with_body_and_history(
        &crate::test_support::parasolid::triangle_body(),
    );
    source.extend(crate::test_support::container::make_block(
        0x43,
        "Contents/Keywords-Custom",
        br#"<Keywords><CustomProperty Name="PartNumber">Authored</CustomProperty></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("fixture decodes");
    let (mut ir, _, fidelity) = decoded.into_parts();
    assert!(ir
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "PartNumber"));
    ir.model
        .attributes
        .retain(|attribute| attribute.name != "PartNumber");
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(&ir, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .expect("deleted neutral attribute writes");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write deleted attribute");
    let written = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("output decodes");
    assert!(!written
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "PartNumber"));
}

#[test]
fn unsupported_sheet_face_does_not_discard_the_other_faces() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube admitted");
    ir.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Sheet;
    ir.model.surfaces[0].geometry =
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None });
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("writable sheet faces remain exportable");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "writer.entity-omitted"
            && loss.severity == cadmpeg_ir::report::Severity::Error));
    let export_losses = plan.report().losses.clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write reduced sheet");
    let written = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("reduced sheet decodes");
    assert_eq!(written.ir().model.bodies.len(), 1);
    assert_eq!(
        written.ir().model.faces.len(),
        5,
        "export losses: {export_losses:?}; decoded losses: {:?}",
        written.report().losses
    );
}

#[test]
fn an_empty_leading_region_keeps_the_bodys_nonempty_boundary() {
    for kind in [
        cadmpeg_ir::topology::BodyKind::Solid,
        cadmpeg_ir::topology::BodyKind::Sheet,
    ] {
        let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube admitted");
        ir.model.bodies[0].kind = kind;
        let region = cadmpeg_ir::topology::Region {
            native_ref: None,
            id: cadmpeg_ir::ids::RegionId::mint("synthetic:test:region#empty").expect("region id"),
            body: ir.model.bodies[0].id.clone(),
            shells: Vec::new(),
        };
        ir.model.bodies[0].regions.insert(0, region.id.clone());
        ir.model.regions.push(region);
        ir.model.finalize();
        let validation =
            cadmpeg_ir::validate::validate_neutral(&ir, Vec::new()).expect("validate document");
        assert!(validation.is_ok(), "{validation:?}");
        let plan = SldprtCodec
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .expect("an empty region has a native null shell head");
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).expect("write regions");
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("native regions parse");
        assert_eq!(decoded.ir().model.bodies.len(), 1);
        assert_eq!(decoded.ir().model.faces.len(), 6);
        assert_eq!(decoded.ir().model.regions.len(), 2);
        assert_eq!(
            decoded
                .ir()
                .model
                .regions
                .iter()
                .filter(|region| region.shells.is_empty())
                .count(),
            1
        );
    }
}

#[test]
fn a_reduced_operation_preserves_its_positional_parameter_quantity() {
    use cadmpeg_ir::features::ParameterValue;
    let mut source = crate::test_support::container::sldprt_with_body(
        &crate::test_support::parasolid::triangle_body(),
    );
    source.extend(crate::test_support::container::make_block(0x42, "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Incomplete extrusion" Type="BossExtrude" id="7"><Dimension Name="D1">1.25</Dimension></Extrusion></Keywords>"#));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("fixture decodes");
    let mut ir = decoded.ir().clone();
    ir.source = None;
    ir.native.0.clear();
    ir.model.configurations.clear();
    for feature in &mut ir.model.features {
        feature.native_ref = None;
    }
    for parameter in &mut ir.model.parameters {
        parameter.native_ref = None;
    }
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("reduced construction does not remove the parameter's quantity");
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "writer.design-reduced"));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write retained scalar");
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("native scalar parses");
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D1")
        .expect("D1 retained");
    assert_eq!(parameter.expression, "1.25".into());
    assert!(matches!(parameter.value, Some(ParameterValue::Length(value)) if value.get() == 1.25));
}

#[test]
fn synthesized_edge_selection_addresses_the_emitted_geometry() {
    use cadmpeg_ir::features::{EdgeSelection, FeatureDefinition, FeatureOperation};
    let mut source = crate::test_support::container::sldprt_with_body(
        &crate::test_support::parasolid::triangle_body(),
    );
    source.extend(crate::test_support::container::make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Fillet Name="Round" Type="Fillet" id="7" Edges="old-selection"><Dimension Name="Radius">2mm</Dimension></Fillet></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("fixture decodes");
    let mut ir = decoded.ir().clone();
    ir.source = None;
    let selected = ir.model.edges[0].id.clone();
    ir.model.features[0].evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) = definition else {
            panic!("typed fillet");
        };
        groups[0].edges = EdgeSelection::Resolved {
            edges: vec![selected.clone()],
            native: "old-selection".into(),
        };
    });
    let points = |ir: &CadIr, edge: &cadmpeg_ir::ids::EdgeId| {
        let edge = ir
            .model
            .edges
            .iter()
            .find(|candidate| &candidate.id == edge)
            .expect("selected edge");
        let mut points = [&edge.start, &edge.end].map(|vertex| {
            let vertex = ir
                .model
                .vertices
                .iter()
                .find(|candidate| &candidate.id == vertex)
                .expect("edge vertex");
            let point = ir
                .model
                .points
                .iter()
                .find(|candidate| candidate.id == vertex.point)
                .expect("vertex point")
                .position()
                .get();
            [point.x, point.y, point.z]
        });
        points.sort_by(|left, right| {
            left[0]
                .total_cmp(&right[0])
                .then(left[1].total_cmp(&right[1]))
                .then(left[2].total_cmp(&right[2]))
        });
        points
    };
    let expected = points(&ir, &selected);
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("resolved selection synthesizes");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write selected geometry");
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("selected geometry parses");
    let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) =
        regenerated.ir().model.features[0].evaluation.definition()
    else {
        panic!("typed fillet retained");
    };
    let EdgeSelection::Resolved { edges, native } = &groups[0].edges else {
        panic!("emitted selection resolves");
    };
    assert_eq!(edges.len(), 1);
    assert_eq!(native, edges[0].as_str());
    assert_eq!(points(regenerated.ir(), &edges[0]), expected);
}

#[test]
fn unheld_neutral_design_keeps_typed_native_history() {
    let source = crate::test_support::history::sldprt_with_body_and_history(
        &crate::test_support::parasolid::triangle_body(),
    );
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("fixture decodes");
    let mut ir = decoded.ir().clone();
    ir.model = cadmpeg_ir::document::Model::default();
    ir.set_scope(cadmpeg_ir::document::ContentScope::container())
        .expect("native-only document scope");
    let before = crate::test_support::native::sldprt_native(&ir);
    assert!(!before.feature_histories[0].features.is_empty());
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("typed native history needs no neutral projection");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write native-only part");
    let written = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("native-only history parses");
    let after = crate::test_support::native::sldprt_native(written.ir());
    let records = |native: &crate::native::SldprtNative| {
        native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .map(|feature| {
                (
                    feature.name.clone(),
                    feature.kind.clone(),
                    feature.parameters.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(records(&after), records(&before));
    assert!(written.ir().model.bodies.is_empty());
}

#[test]
fn design_only_native_custom_property_remains_native() {
    let mut source = crate::test_support::container::sldprt_with_body(
        &crate::test_support::parasolid::triangle_body(),
    );
    source.extend(crate::test_support::container::make_block(
        0x43,
        "Contents/Keywords-Custom",
        br#"<Keywords><CustomProperty Name="Author">Ada</CustomProperty></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    ir.model = cadmpeg_ir::document::Model::default();
    ir.set_scope(cadmpeg_ir::document::ContentScope::container())
        .unwrap();
    let before = crate::test_support::native::sldprt_native(&ir);
    assert!(before
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .any(|feature| feature.xml_tag == "CustomProperty"));
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = crate::test_support::native::sldprt_native(output.ir());
    assert!(native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .any(|feature| feature.xml_tag == "CustomProperty"
            && feature.name == "Author"
            && feature.text.as_deref() == Some("Ada")));
    assert!(!output.ir().scope.model);
}

#[test]
fn design_only_native_pmi_remains_native() {
    let mut source = crate::test_support::container::sldprt_with_body(
        &crate::test_support::parasolid::triangle_body(),
    );
    source.extend(crate::test_support::container::make_block(
        0x43,
        "Contents/PMISemanticDataDB",
        &crate::test_support::pmi::pmi_semantic_payload(),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    ir.model = cadmpeg_ir::document::Model::default();
    ir.set_scope(cadmpeg_ir::document::ContentScope::container())
        .unwrap();
    let before = crate::test_support::native::sldprt_native(&ir);
    assert!(!before.pmi_dimensions.is_empty());
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    for record in &before.pmi_dimensions {
        assert_ne!(
            plan.report().native_dispositions.record_disposition(
                "sldprt",
                "pmi_dimensions",
                &record.id
            ),
            cadmpeg_ir::codec::write::NativeDisposition::Dropped
        );
    }
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = crate::test_support::native::sldprt_native(output.ir());
    assert_eq!(native.pmi_dimensions.len(), before.pmi_dimensions.len());
    for expected in &before.pmi_dimensions {
        assert!(native.pmi_dimensions.iter().any(|actual| {
            actual.guid == expected.guid
                && actual.value == expected.value
                && actual.cad_text == expected.cad_text
        }));
    }
    assert!(!output.ir().scope.model);
}

#[test]
fn unheld_neutral_model_keeps_supported_typed_native_document_metadata() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    let attribute = SourceAttribute {
        native_ref: None,
        id: cadmpeg_ir::ids::AttributeId::mint("sldprt:metadata:source_linear_unit_code#0:0")
            .expect("metadata id"),
        target: AttributeTarget::Document,
        name: "source_linear_unit_code".into(),
        values: vec![Some(AttributeValue::Integer(0))],
    };
    let mut ir = CadIr::empty();
    ir.set_scope(cadmpeg_ir::document::ContentScope::container())
        .expect("native-only document scope");
    crate::native::SldprtNative {
        document_metadata: vec![crate::native::DocumentMetadata {
            id: cadmpeg_core::nonblank_literal!("sldprt:metadata:document#0"),
            attributes: vec![attribute],
            pmi: Vec::new(),
            semantic_annotations: Vec::new(),
        }],
        ..crate::native::SldprtNative::default()
    }
    .store(
        &cadmpeg_test_support::service_decode_context(),
        ir.native.namespace_mut("sldprt"),
    )
    .expect("native metadata admitted");
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("native metadata has a known encoding");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write native metadata");
    let written = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("native metadata parses");
    let native = crate::test_support::native::sldprt_native(written.ir());
    assert!(native
        .document_metadata
        .iter()
        .flat_map(|record| &record.attributes)
        .any(|attribute| attribute.name == "source_linear_unit_code"
            && attribute.values == [Some(AttributeValue::Integer(0))]));
}

#[test]
fn single_segment_composite_edges_point_at_their_leaf_carrier() {
    use cadmpeg_ir::geometry::{
        CompositeCurveSegment, CompositeCurveSegments, CompositeCurveTransition, CurveGeometry,
        SolvedCurveGeometry,
    };
    use cadmpeg_ir::ids::CurveId;
    use cadmpeg_ir::topology::EdgeCarrier;

    let mut ir = crate::test_support::ir::source_less_cube();
    let leaf = ir.model.curves[0].clone();
    let edge_curve = ir.model.edges[0]
        .curve()
        .expect("cube edge has a carrier")
        .clone();
    assert_eq!(edge_curve, leaf.id);
    let segment = |curve: CurveId| CompositeCurveSegment {
        curve,
        same_sense: true,
        transition: CompositeCurveTransition::Continuous,
    };
    let mut inner = leaf.clone();
    inner.id = CurveId::mint("sldprt:brep:curve#composite-inner").expect("composite id mints");
    inner.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Composite {
        segments: CompositeCurveSegments::try_from(vec![segment(leaf.id.clone())])
            .expect("one segment admitted"),
        self_intersect: Some(false),
    });
    let mut outer = leaf.clone();
    outer.id = CurveId::mint("sldprt:brep:curve#composite-outer").expect("composite id mints");
    outer.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Composite {
        segments: CompositeCurveSegments::try_from(vec![segment(inner.id.clone())])
            .expect("one segment admitted"),
        self_intersect: Some(false),
    });
    ir.model.curves.push(inner);
    ir.model.curves.push(outer);
    let outer_id = CurveId::mint("sldprt:brep:curve#composite-outer").expect("composite id mints");
    ir.model.edges[0].map_curve(|_| outer_id);
    assert!(matches!(ir.model.edges[0].carrier, EdgeCarrier::Curve(_)));

    let mut losses = Vec::new();
    super::geometry::flatten_composite_curves(&mut ir, &mut losses).unwrap();
    assert_eq!(ir.model.edges[0].curve(), Some(&leaf.id));
    assert!(ir.model.curves.iter().all(|curve| !matches!(
        curve.geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Composite { .. })
    )));
    assert_eq!(losses.len(), 2);
    assert!(losses
        .iter()
        .all(|loss| loss.code.local_code() == "writer.entity-omitted"));
    assert!(losses
        .iter()
        .any(|loss| loss.message.contains("sldprt:brep:curve#composite-inner")));
    assert!(losses
        .iter()
        .any(|loss| loss.message.contains("sldprt:brep:curve#composite-outer")));
}

#[test]
fn unused_composite_is_written_as_its_segments() {
    use cadmpeg_ir::geometry::{
        CompositeCurveSegment, CompositeCurveTransition, CurveGeometry, SolvedCurveGeometry,
    };
    use cadmpeg_ir::ids::CurveId;

    let mut ir = crate::test_support::ir::source_less_cube();
    let first = ir.model.curves[0].clone();
    let second = ir.model.curves[1].clone();
    let id = CurveId::mint("sldprt:brep:curve#composite-unused").unwrap();
    let mut composite = first.clone();
    composite.id = id.clone();
    composite.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Composite {
        segments: [first.id.clone(), second.id.clone()]
            .into_iter()
            .map(|curve| CompositeCurveSegment {
                curve,
                same_sense: true,
                transition: CompositeCurveTransition::Continuous,
            })
            .collect::<Vec<_>>()
            .try_into()
            .unwrap(),
        self_intersect: Some(false),
    });
    ir.model.curves.push(composite);
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(id.as_str())));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    for segment in [first, second] {
        assert!(decoded
            .ir()
            .model
            .curves
            .iter()
            .any(|curve| curve.geometry == segment.geometry));
    }
}

#[test]
fn cyclic_composites_are_not_reported_as_written_segments() {
    use cadmpeg_ir::geometry::{
        CompositeCurveSegment, CompositeCurveTransition, CurveGeometry, SolvedCurveGeometry,
    };
    use cadmpeg_ir::ids::CurveId;

    let mut ir = crate::test_support::ir::source_less_cube();
    let mut curve = ir.model.curves[0].clone();
    curve.id = CurveId::mint("sldprt:brep:curve#composite-cycle").unwrap();
    curve.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Composite {
        segments: vec![CompositeCurveSegment {
            curve: curve.id.clone(),
            same_sense: true,
            transition: CompositeCurveTransition::Continuous,
        }]
        .try_into()
        .unwrap(),
        self_intersect: Some(false),
    });
    let id = curve.id.clone();
    ir.model.curves.push(curve);
    let mut losses = Vec::new();
    super::geometry::flatten_composite_curves(&mut ir, &mut losses).unwrap();
    assert!(ir.model.curves.iter().any(|curve| curve.id == id));
    assert!(losses.is_empty());
}

#[test]
fn nested_composites_used_by_an_edge_keep_their_dependencies() {
    use cadmpeg_ir::geometry::{
        CompositeCurveSegment, CompositeCurveTransition, CurveGeometry, SolvedCurveGeometry,
    };
    use cadmpeg_ir::ids::CurveId;

    let mut ir = crate::test_support::ir::source_less_cube();
    let leaf = ir.model.curves[0].clone();
    let segment = |curve| CompositeCurveSegment {
        curve,
        same_sense: true,
        transition: CompositeCurveTransition::Continuous,
    };
    let inner_id = CurveId::mint("sldprt:brep:curve#composite-nested").unwrap();
    let outer_id = CurveId::mint("sldprt:brep:curve#composite-carrier").unwrap();
    let mut inner = leaf.clone();
    inner.id = inner_id.clone();
    inner.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Composite {
        segments: vec![segment(leaf.id.clone())].try_into().unwrap(),
        self_intersect: Some(false),
    });
    let mut outer = leaf.clone();
    outer.id = outer_id.clone();
    outer.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Composite {
        segments: vec![segment(inner_id.clone()), segment(leaf.id)]
            .try_into()
            .unwrap(),
        self_intersect: Some(false),
    });
    ir.model.curves.extend([inner, outer]);
    ir.model.edges[0].map_curve(|_| outer_id.clone());
    let mut losses = Vec::new();
    super::geometry::flatten_composite_curves(&mut ir, &mut losses).unwrap();
    assert!(ir.model.curves.iter().any(|curve| curve.id == outer_id));
    assert!(ir.model.curves.iter().any(|curve| curve.id == inner_id));
    assert!(losses.is_empty());
}

#[test]
fn multi_segment_composites_keep_their_refusal() {
    use cadmpeg_ir::geometry::{
        CompositeCurveSegment, CompositeCurveSegments, CompositeCurveTransition, CurveGeometry,
        SolvedCurveGeometry,
    };
    use cadmpeg_ir::ids::CurveId;
    use cadmpeg_ir::topology::EdgeCarrier;

    let mut ir = crate::test_support::ir::source_less_cube();
    let leaf = ir.model.curves[0].clone();
    let segment = |curve: CurveId| CompositeCurveSegment {
        curve,
        same_sense: true,
        transition: CompositeCurveTransition::Continuous,
    };
    let mut multi = leaf.clone();
    multi.id = CurveId::mint("sldprt:brep:curve#composite-multi").expect("composite id mints");
    multi.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Composite {
        segments: CompositeCurveSegments::try_from(vec![
            segment(leaf.id.clone()),
            segment(leaf.id.clone()),
        ])
        .expect("two segments admitted"),
        self_intersect: Some(false),
    });
    ir.model.curves.push(multi);
    let multi_id = CurveId::mint("sldprt:brep:curve#composite-multi").expect("composite id mints");
    ir.model.edges[0].map_curve(|_| multi_id);
    assert!(matches!(ir.model.edges[0].carrier, EdgeCarrier::Curve(_)));

    let mut losses = Vec::new();
    super::geometry::flatten_composite_curves(&mut ir, &mut losses).unwrap();
    assert!(ir.model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Composite { .. })
    )));
    assert!(losses.is_empty());
}

#[test]
fn incoherent_edge_endpoints_are_carried_as_stated() {
    use cadmpeg_ir::topology::ParameterInterval;

    let mut ir = crate::test_support::ir::source_less_cube();
    let edge_id = ir.model.edges[0].id.clone();
    ir.model.edges[0].set_param_range(Some(
        ParameterInterval::new([0.0, 1.0]).expect("unit range admitted"),
    ));
    let end = ir.model.edges[0].end.clone();
    let point = ir
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == end)
        .expect("end vertex held")
        .point
        .clone();
    let record = ir
        .model
        .points
        .iter_mut()
        .find(|record| record.id == point)
        .expect("end point held");
    let moved = record.position().get();
    record.set_position(
        cadmpeg_ir::features::FinitePoint3::new([moved.x + 5.0, moved.y, moved.z].into())
            .expect("moved point is finite"),
    );
    let validation =
        cadmpeg_ir::validate::validate_neutral(&ir, Vec::new()).expect("neutral validation runs");
    assert!(
        validation.findings.iter().any(|finding| finding
            .message
            .starts_with("edge curve endpoints miss the edge's vertex positions")
            && finding.entity.as_deref() == Some(edge_id.as_str())),
        "the moved vertex is genuinely incoherent with its curve"
    );
    let mut losses = Vec::new();
    super::cascade_validation_errors(&mut ir, &mut losses)
        .expect("an incoherent edge does not refuse the write");
    assert!(
        ir.model.edges.iter().any(|edge| edge.id == edge_id),
        "the incoherent edge is carried, not dropped"
    );
}

#[test]
fn inconsistent_pcurve_does_not_refuse_the_other_geometry() {
    use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata};
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::topology::{ParameterInterval, PcurveUse};

    let mut ir = crate::test_support::ir::source_less_cube();
    let edge_id = ir.model.coedges[0].edge.clone();
    ir.model
        .edges
        .iter_mut()
        .find(|edge| edge.id == edge_id)
        .unwrap()
        .set_param_range(Some(ParameterInterval::new([0.0, 1.0]).unwrap()));
    let pcurve = Pcurve {
        native_ref: None,
        id: "synthetic:test:pcurve#inconsistent".try_into().unwrap(),
        geometry: PcurveGeometry::Line(
            LinePcurve::try_new(Point2::new(1000.0, 1000.0), Point2::new(1.0, 0.0)).unwrap(),
        ),
        metadata: PcurveMetadata::default(),
    };
    let coedge_id = ir.model.coedges[0].id.clone();
    ir.model.coedges[0].pcurves.push(PcurveUse {
        pcurve: pcurve.id.clone(),
        isoparametric: None,
        parameter_range: None,
    });
    ir.model.pcurves.push(pcurve);
    let validation = cadmpeg_ir::validate::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(validation.findings.iter().any(|finding| {
        finding
            .message
            .starts_with("pcurve mapped through the face surface misses")
            && finding.entity.as_deref() == Some(coedge_id.as_str())
    }));
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("inconsistent pcurve does not refuse the model");
    assert!(plan.report().losses.iter().any(|loss| {
        loss.code == crate::loss::SldprtLossCode::WriterCoedgePcurveUsesOmitted.kind()
            && loss.message.contains(coedge_id.as_str())
    }));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.faces.len(), ir.model.faces.len());
}

#[test]
fn loopless_sphere_is_written_as_a_whole_face() {
    let source = crate::test_support::container::sldprt_with_body(
        &crate::test_support::parasolid::sphere_patch_body(),
    );
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    assert_eq!(ir.model.faces.len(), 1);
    ir.model.faces[0].loops = cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new());
    ir.model.loops.clear();
    ir.model.coedges.clear();
    ir.model.edges.clear();
    ir.model.vertices.clear();
    ir.model.points.clear();
    ir.model.curves.clear();
    ir.model.pcurves.clear();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("whole sphere is writable");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.faces.len(), 1);
    assert!(output.ir().model.faces[0].loops.is_empty());
    assert!(matches!(
        output.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_))
    ));
}

#[test]
fn unsupported_loopless_face_reports_its_body() {
    let source = crate::test_support::container::sldprt_with_body(
        &crate::test_support::parasolid::triangle_body(),
    );
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    let body = ir.model.bodies[0].id.clone();
    let cascaded = [
        ir.model.regions[0].id.as_str(),
        ir.model.shells[0].id.as_str(),
        ir.model.faces[0].id.as_str(),
    ]
    .map(str::to_owned);
    ir.model.faces[0].loops = cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new());
    ir.model.loops.clear();
    ir.model.coedges.clear();
    ir.model.edges.clear();
    ir.model.vertices.clear();
    ir.model.points.clear();
    ir.model.curves.clear();
    ir.model.pcurves.clear();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("unsupported geometry is reported");
    assert!(plan.report().losses.iter().any(|loss| {
        loss.code == crate::loss::SldprtLossCode::WriterEntityOmitted.kind()
            && loss.message.contains(body.as_str())
    }));
    for id in cascaded {
        assert!(
            plan.report().losses.iter().any(|loss| {
                loss.code == crate::loss::SldprtLossCode::WriterEntityOmitted.kind()
                    && loss.message.contains(&id)
            }),
            "cascade loss for {id}"
        );
    }
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the remaining document decodes");
}

#[test]
fn omitted_body_reports_each_owned_topology_record() {
    let mut ir = crate::test_support::ir::source_less_cube();
    ir.model.surfaces[0].geometry =
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None });
    let ids: Vec<String> = ir
        .model
        .regions
        .iter()
        .map(|record| record.id.to_string())
        .chain(ir.model.shells.iter().map(|record| record.id.to_string()))
        .chain(ir.model.faces.iter().map(|record| record.id.to_string()))
        .chain(ir.model.loops.iter().map(|record| record.id.to_string()))
        .chain(ir.model.coedges.iter().map(|record| record.id.to_string()))
        .collect();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("the unsupported body is omitted");
    for id in ids {
        assert!(
            plan.report().losses.iter().any(|loss| {
                loss.code == crate::loss::SldprtLossCode::WriterEntityOmitted.kind()
                    && loss.message.contains(&id)
            }),
            "cascade loss for {id}"
        );
    }
}

#[test]
fn wire_body_is_reported_without_discarding_a_solid() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("cube admitted");
    let wire_id = cadmpeg_ir::ids::BodyId::mint("synthetic:test:body#wire").expect("body id");
    ir.model.bodies.push(cadmpeg_ir::topology::Body {
        native_ref: None,
        id: wire_id.clone(),
        kind: cadmpeg_ir::topology::BodyKind::Wire,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("solid remains exportable");
    assert!(plan.report().losses.iter().any(|loss| {
        loss.code.local_code() == "writer.entity-omitted" && loss.message.contains(wire_id.as_str())
    }));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write solid");
    let written = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("output decodes");
    assert_eq!(written.ir().model.bodies.len(), 1);
    assert_eq!(
        written.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Solid
    );
}

#[test]
fn foreign_document_string_is_written_as_a_custom_property() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    let mut ir = CadIr::empty();
    ir.model.attributes.push(SourceAttribute {
        id: cadmpeg_ir::ids::AttributeId::mint("iges:application:attribute#document:author")
            .unwrap(),
        native_ref: None,
        target: AttributeTarget::Document,
        name: "author".into(),
        values: vec![Some(AttributeValue::String("author".into()))],
    });
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = crate::test_support::native::sldprt_native(output.ir());
    assert!(native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .any(|feature| {
            feature.xml_tag == "CustomProperty"
                && feature.name == "author"
                && feature.text.as_deref() == Some("author")
        }));
    assert!(output.ir().model.attributes.iter().any(|attribute| {
        attribute.target == AttributeTarget::Document
            && attribute.name == "author"
            && attribute.values == vec![Some(AttributeValue::String("author".into()))]
    }));
}

#[test]
fn inactive_configuration_writes_real_parameter_value() {
    use cadmpeg_ir::features::ParameterValue;
    use cadmpeg_ir::scalar::FiniteReal;

    let mut source = crate::test_support::container::sldprt_with_body(
        &crate::test_support::parasolid::triangle_body(),
    );
    source.extend(crate::test_support::container::make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Configuration Name="Large"/><Scale Name="Scale1" Type="Scale" id="7"><Dimension Name="Factor">1.5</Dimension></Scale></Keywords>"#,
    ));
    let source = crate::test_support::container::native_envelope_fixture(&source);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    let active_name = ir
        .model
        .configurations
        .iter()
        .find(|c| c.active)
        .unwrap()
        .name
        .clone();
    let parameter = ir
        .model
        .parameters
        .iter()
        .find(|p| p.name == "Factor")
        .unwrap();
    let id = parameter.id.clone();
    let expected = ParameterValue::Real(FiniteReal::new(2.5).unwrap());
    let inactive = ir
        .model
        .configurations
        .iter_mut()
        .find(|c| !c.active)
        .unwrap();
    let inactive_name = inactive.name.clone();
    inactive
        .parameter_overrides
        .insert(id.clone(), cadmpeg_ir::features::Expression::text("2.5"));
    inactive.parameter_values.insert(id, expected.clone());
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = crate::test_support::native::sldprt_native(output.ir());
    for (name, value) in [(active_name, 1.5), (inactive_name.clone(), 2.5)] {
        let configuration = output
            .ir()
            .model
            .configurations
            .iter()
            .find(|c| c.name == name)
            .unwrap();
        let slot = crate::history::write::configurations::configuration_slot(
            &configuration.properties,
            configuration.ordinal,
        )
        .to_string();
        assert!(
            native.feature_input_lanes.iter().any(|lane| {
                lane.configuration.as_deref() == Some(slot.as_str())
                    && lane
                        .scalars
                        .iter()
                        .any(|scalar| scalar.value.get() == value)
            }),
            "configuration {slot} has scalar {value}"
        );
    }
    let parameter = output
        .ir()
        .model
        .parameters
        .iter()
        .find(|p| p.name == "Factor")
        .unwrap();
    let inactive = output
        .ir()
        .model
        .configurations
        .iter()
        .find(|c| c.name == inactive_name)
        .unwrap();
    assert_eq!(
        parameter.value,
        Some(ParameterValue::Real(FiniteReal::new(1.5).unwrap()))
    );
    assert_eq!(
        inactive.parameter_values.get(&parameter.id),
        Some(&expected)
    );
    assert_eq!(
        inactive.parameter_value(output.ir(), &parameter.id),
        Some(&expected)
    );
    assert_eq!(
        inactive.parameter_overrides.get(&parameter.id),
        Some(&cadmpeg_ir::features::Expression::text("2.5"))
    );
}

#[test]
fn solid_body_with_six_regions_survives_synthesis() {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let owner = ir.model.bodies[0].id.clone();
    for index in 1..6 {
        let source = serde_json::to_string(&cadmpeg_ir::examples::unit_cube().unwrap())
            .unwrap()
            .replace("synthetic:cube:", &format!("synthetic:cube{index}:"));
        let mut next: CadIr = serde_json::from_str(&source).unwrap();
        ir.model.bodies[0]
            .regions
            .extend(next.model.bodies[0].regions.clone());
        for region in &mut next.model.regions {
            region.body = owner.clone();
        }
        macro_rules! append {
            ($($arena:ident),* $(,)?) => {$(ir.model.$arena.extend(next.model.$arena);)*};
        }
        append!(regions, shells, faces, loops, coedges, edges, vertices, points, surfaces, curves);
    }
    super::super::sort_arenas(&mut ir);
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.bodies.len(), 1);
    assert_eq!(output.ir().model.bodies[0].regions.len(), 6);
    assert_eq!(output.ir().model.faces.len(), 36);
}

#[test]
fn inactive_configuration_retains_its_sketch_feature_owner() {
    use cadmpeg_ir::features::{
        ConfigurationFeatureState, ConfigurationId, DesignConfiguration, FeatureDefinition,
        FeatureOperation, SketchFeatureBinding,
    };
    let source = crate::test_support::history::sldprt_with_nested_sketch_profile(
        &crate::test_support::parasolid::triangle_body(),
    );
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    let mut base = ir.model.sketches[0].clone();
    base.native_ref = None;
    let feature = cadmpeg_ir::features::FeatureId::mint("synthetic:test:feature#sketch").unwrap();
    let ordinal = ir
        .model
        .features
        .iter()
        .map(|feature| feature.ordinal)
        .max()
        .unwrap_or(0)
        + 1;
    ir.model.features.push(cadmpeg_ir::features::Feature {
        product_definition: None,
        id: feature.clone(),
        ordinal,
        name: Some("Sketch".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        needs_recompute: false,
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(base.id.clone())),
            }),
        ),
        native_ref: None,
    });
    let mut ids = std::collections::HashMap::new();
    ids.insert(
        base.id.to_string(),
        "synthetic:test:sketch#alternate".to_owned(),
    );
    let mut entities = ir
        .model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == base.id)
        .cloned()
        .collect::<Vec<_>>();
    for entity in &mut entities {
        entity.native_ref = None;
        entity.geometry_ref = None;
        entity.endpoint_refs.clear();
    }
    for (index, entity) in entities.iter().enumerate() {
        ids.insert(
            entity.id().to_string(),
            format!("synthetic:test:sketch-entity#alternate-{index}"),
        );
    }
    let context = cadmpeg_test_support::service_decode_context();
    let mut alternate = cadmpeg_ir::schema::rewrite::identities(&context, base, |id| {
        Ok(ids.get(id).cloned().unwrap_or_else(|| id.to_owned()))
    })
    .unwrap();
    alternate.configuration = Some("1".into());
    alternate.native_ref = None;
    let alternate_id = alternate.id.clone();
    let mut alternate_entities =
        cadmpeg_ir::schema::rewrite::identities(&context, entities, |id| {
            Ok(ids.get(id).cloned().unwrap_or_else(|| id.to_owned()))
        })
        .unwrap();
    for entity in &mut alternate_entities {
        entity.native_ref = None;
        entity.geometry_ref = None;
        entity.endpoint_refs.clear();
    }
    let expected_count = alternate_entities.len();
    let expected_line = alternate_entities
        .iter()
        .find_map(|entity| match entity.geometry.definition() {
            cadmpeg_ir::sketches::SketchGeometryDefinition::Line { start, end } => {
                Some((*start, *end))
            }
            _ => None,
        })
        .unwrap();
    ir.model.sketches.push(alternate);
    ir.model.sketch_entities.extend(alternate_entities);
    let bodies = cadmpeg_ir::features::DistinctMembers::try_from(
        ir.model
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    ir.model.configurations = (0..2)
        .map(|index| DesignConfiguration {
            id: ConfigurationId::mint(format!("synthetic:test:configuration#{index}")).unwrap(),
            ordinal: index,
            active: index == 0,
            source_index: Some(index),
            name: Some(format!("Configuration {index}")),
            material: None,
            properties: std::collections::BTreeMap::default(),
            bodies: Some(bodies.clone()),
            parameter_values: std::collections::BTreeMap::default(),
            parameter_overrides: std::collections::BTreeMap::default(),
            feature_states: std::collections::BTreeMap::default(),
            native_ref: None,
        })
        .collect();
    ir.model.configurations[1].feature_states.insert(
        feature.clone(),
        ConfigurationFeatureState::Definition {
            definition: Box::new(FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(alternate_id)),
            })),
        },
    );
    let parameter =
        cadmpeg_ir::features::ParameterId::mint("synthetic:test:parameter#sketch-length").unwrap();
    ir.model
        .parameters
        .push(cadmpeg_ir::features::DesignParameter {
            product_definition: None,
            id: parameter.clone(),
            owner: Some(feature.clone()),
            ordinal: 0,
            name: "D1".into(),
            expression: cadmpeg_ir::features::Expression::text("10mm"),
            driving: true,
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Length(
                cadmpeg_ir::scalar::Length::new(10.0).unwrap(),
            )),
            properties: std::collections::BTreeMap::default(),
            pmi: None,
            native_ref: None,
        });
    ir.model.configurations[1].parameter_overrides.insert(
        parameter.clone(),
        cadmpeg_ir::features::Expression::text("20mm"),
    );
    ir.model.configurations[1].parameter_values.insert(
        parameter,
        cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(20.0).unwrap(),
        ),
    );
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(feature.as_str())
            && loss.message.contains("structural override")));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let owners = output
        .ir()
        .model
        .features
        .iter()
        .filter(|item| {
            matches!(
                item.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Sketch { .. })
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(owners.len(), 1);
    let configuration = output
        .ir()
        .model
        .configurations
        .iter()
        .find(|configuration| configuration.name.as_deref() == Some("Configuration 1"))
        .unwrap();
    let parameter = output
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D1" && parameter.owner.as_ref() == Some(&owners[0].id))
        .unwrap();
    assert_eq!(
        configuration.parameter_value(output.ir(), &parameter.id),
        Some(&cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(20.0).unwrap()
        ))
    );
    let Some(ConfigurationFeatureState::Definition { definition }) =
        configuration.feature_states.get(&owners[0].id)
    else {
        panic!("alternate state is bound to its owner")
    };
    let FeatureDefinition::Operation(FeatureOperation::Sketch {
        sketch: SketchFeatureBinding::Planar(Some(sketch)),
    }) = definition.as_ref()
    else {
        panic!("alternate state carries a sketch")
    };
    let written = output
        .ir()
        .model
        .sketch_entities
        .iter()
        .filter(|entity| &entity.sketch == sketch)
        .collect::<Vec<_>>();
    assert_eq!(written.len(), expected_count);
    assert!(written
        .iter()
        .any(|entity| match entity.geometry.definition() {
            cadmpeg_ir::sketches::SketchGeometryDefinition::Line { start, end } => {
                (start.u - expected_line.0.u).abs() < 1.0e-8
                    && (start.v - expected_line.0.v).abs() < 1.0e-8
                    && (end.u - expected_line.1.u).abs() < 1.0e-8
                    && (end.v - expected_line.1.v).abs() < 1.0e-8
            }
            _ => false,
        }));
}

#[test]
fn periodic_nurbs_surface_is_written_and_redecoded() {
    use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
    use cadmpeg_ir::topology::Sense;

    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let face = ir.model.faces[0].clone();
    let loop_id = face.loops.iter().next().unwrap();
    let boundary = ir.model.loops.iter().find(|lp| &lp.id == loop_id).unwrap();
    let corners = boundary
        .coedges()
        .iter()
        .map(|id| {
            let coedge = ir
                .model
                .coedges
                .iter()
                .find(|coedge| &coedge.id == id)
                .unwrap();
            let edge = ir
                .model
                .edges
                .iter()
                .find(|edge| edge.id == coedge.edge)
                .unwrap();
            let vertex_id = if coedge.sense == Sense::Forward {
                &edge.start
            } else {
                &edge.end
            };
            let vertex = ir
                .model
                .vertices
                .iter()
                .find(|vertex| &vertex.id == vertex_id)
                .unwrap();
            ir.model
                .points
                .iter()
                .find(|point| point.id == vertex.point)
                .unwrap()
                .position()
        })
        .collect::<Vec<_>>();
    assert_eq!(corners.len(), 4);
    let surface = NurbsSurface::new(
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 0.5, 1.0, 1.0], true),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsPoleGrid::Polynomial {
            rows: vec![
                vec![corners[0], corners[3]],
                vec![corners[1], corners[2]],
                vec![corners[0], corners[3]],
            ],
        },
        false,
    )
    .unwrap();
    let expected = surface.clone();
    ir.model
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == face.surface)
        .unwrap()
        .geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface));
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output.ir().model.surfaces.iter().any(|surface| matches!(&surface.geometry,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) if nurbs.u_periodic() && !nurbs.v_periodic()
            && nurbs.u_knots() == expected.u_knots() && nurbs.v_knots() == expected.v_knots()
            && nurbs.control_grid() == expected.control_grid()
    )));
}

#[test]
fn periodic_nurbs_curve_is_written_and_redecoded() {
    use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoles3};
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};

    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let edge = ir.model.edges[0].clone();
    let point = |vertex_id: &cadmpeg_ir::ids::VertexId| {
        let vertex = ir
            .model
            .vertices
            .iter()
            .find(|v| &v.id == vertex_id)
            .unwrap();
        ir.model
            .points
            .iter()
            .find(|p| p.id == vertex.point)
            .unwrap()
            .position()
            .get()
    };
    let start = point(&edge.start);
    let end = point(&edge.end);
    let poles = NurbsPoles3::from_lanes(vec![start, end, start], None).unwrap();
    let curve = NurbsCurve::new(1, vec![0.0, 0.0, 0.5, 1.0, 1.0], poles, true).unwrap();
    let mut carrier = Vec::new();
    crate::writer::write_nurbs_curve(&mut carrier, 10, &curve, &mut 20, 0.001).unwrap();
    let descriptor = carrier.windows(2).position(|tag| tag == [0, 0x88]).unwrap() + 2;
    assert_eq!(
        cadmpeg_core::decode::View::u16_be_at(
            &carrier,
            descriptor + crate::layout::bspline_curve_descriptor::PERIODIC
        ),
        Some(0x0100)
    );
    ir.model
        .curves
        .iter_mut()
        .find(|curve| Some(&curve.id) == edge.curve())
        .unwrap()
        .geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve));
    ir.model.edges[0].set_param_range(Some(
        cadmpeg_ir::topology::ParameterInterval::new([0.0, 0.5]).unwrap(),
    ));
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(output.ir().model.curves.iter().any(|curve| matches!(&curve.geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) if nurbs.periodic()
            && nurbs.control_points().iter().map(|p| p.get()).collect::<Vec<_>>() == [start, end, start]
    )), "{:#?}", output.ir().model.curves);
}

#[test]
fn polyline_carrier_is_written_as_a_degree_one_nurbs() {
    use cadmpeg_ir::geometry::sampled::{PolylineCurve, PolylineSamples};
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let edge = ir.model.edges[0].clone();
    let point = |vertex_id: &cadmpeg_ir::ids::VertexId| {
        let vertex = ir
            .model
            .vertices
            .iter()
            .find(|v| &v.id == vertex_id)
            .unwrap();
        ir.model
            .points
            .iter()
            .find(|p| p.id == vertex.point)
            .unwrap()
            .position()
            .get()
    };
    let start = point(&edge.start);
    let end = point(&edge.end);
    let middle = Point3::new(
        (start.x + end.x) / 2.0,
        (start.y + end.y) / 2.0,
        (start.z + end.z) / 2.0,
    );
    let polyline = PolylineCurve::new(
        PolylineSamples::Unparameterized {
            points: vec![start, middle, end].try_into().unwrap(),
        },
        0.0,
    )
    .unwrap();
    let curve = ir
        .model
        .curves
        .iter_mut()
        .find(|curve| Some(&curve.id) == edge.curve())
        .unwrap();
    let curve_id = curve.id.clone();
    curve.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Polyline(polyline));
    curve.parameter_range = None;
    ir.model.edges[0].set_param_range(None);
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "writer.polyline-converted"
            && loss.message.contains(curve_id.as_str())));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.bodies.len(), 1);
    assert!(output.ir().model.curves.iter().any(|curve| matches!(&curve.geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs))
            if nurbs.degree() == 1 && nurbs.knots().as_slice() == [0.0, 0.0, 1.0, 2.0, 2.0]
                && nurbs.control_points().iter().map(|point| point.get()).collect::<Vec<_>>() == [start, middle, end]
    )));
}
