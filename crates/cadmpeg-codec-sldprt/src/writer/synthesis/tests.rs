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
fn unheld_neutral_model_keeps_supported_typed_native_document_metadata() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    let attribute = SourceAttribute {
        id: cadmpeg_ir::ids::AttributeId::mint("sldprt:metadata:source_linear_unit_code#0:0")
            .expect("metadata id"),
        target: AttributeTarget::Document,
        name: "source_linear_unit_code".into(),
        values: vec![AttributeValue::Integer(0)],
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
            && attribute.values == [AttributeValue::Integer(0)]));
}
