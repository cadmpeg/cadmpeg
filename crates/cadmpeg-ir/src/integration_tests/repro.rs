// SPDX-License-Identifier: Apache-2.0
use crate::repro::*;
use crate::CadIr;
use crate::{math::Point3, report::check::Check};

fn failing_cube() -> (CadIr, crate::report::check::Finding) {
    let mut document = crate::examples::unit_cube().unwrap();
    document.model.points[0].set_position(Point3::new(50.0, 0.0, 0.0).try_into().unwrap());
    let finding = crate::validate_neutral(&document, Vec::new())
        .unwrap()
        .findings
        .into_iter()
        .find(|finding| finding.check == Check::GeometricConsistency)
        .unwrap();
    (document, finding)
}

#[test]
fn repro_closes_edge_and_shell_context_and_round_trips_document() {
    let (document, finding) = failing_cube();
    let bundle = cut(&document, &finding, &CutOptions::default()).unwrap();
    assert_eq!(bundle.document.model.shells.len(), 1);
    assert_eq!(bundle.document.model.faces.len(), 6);
    assert_eq!(bundle.document.model.coedges.len(), 24);
    assert_eq!(bundle.document.model.edges.len(), 12);
    assert_eq!(bundle.document.model.vertices.len(), 8);
    assert_eq!(bundle.document.model.points.len(), 8);
    let report = crate::validate_neutral(&bundle.document, Vec::new()).unwrap();
    assert!(report
        .findings
        .iter()
        .any(|finding| bundle.signature.reproduces(finding)));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.check == Check::FragmentSkipped));
    let parsed: CadIr =
        serde_json::from_slice(&serde_json::to_vec(&bundle.document).unwrap()).unwrap();
    assert_eq!(parsed, bundle.document);
    assert!(parsed.fragment);
}

#[test]
fn repro_strips_names_paths_ids_and_metadata_and_preserves_context() {
    let (mut document, finding) = failing_cube();
    document.model.bodies[0].name = Some("PLANTED_NAME".into());
    let mut value = serde_json::to_value(&document).unwrap();
    value["source"] = serde_json::json!({"identity": {"classification":"unclassified","format":"f3d"},
        "attributes": {"PLANTED_METADATA_KEY":"PLANTED_METADATA_VALUE", "file_path":"/PLANTED_FILE/source.cad"}});
    value["model"]["points"][0]["source_object"] = serde_json::json!({"format":"f3d", "object_id":"PLANTED_SOURCE_NAME",
        "name":"PLANTED_OBJECT_NAME", "layer":"PLANTED_LAYER", "instance_path":["PLANTED_MEMBER_PATH/member.cad"]});

    let old = value["model"]["points"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let text = serde_json::to_string(&value)
        .unwrap()
        .replace(&old, "synthetic:PLANTED_PATH/file.cad:point#1");
    value = serde_json::from_str(&text).unwrap();
    document = serde_json::from_value(value).unwrap();
    let options = CutOptions {
        context: serde_json::from_value(serde_json::json!({"operation": ["opaque", 42]})).unwrap(),
        ..CutOptions::default()
    };
    let bundle = cut(&document, &finding, &options).unwrap();
    let json = serde_json::to_string(&bundle).unwrap();
    for secret in [
        "PLANTED_NAME",
        "PLANTED_PATH",
        "file.cad",
        "PLANTED_METADATA_KEY",
        "PLANTED_METADATA_VALUE",
        "PLANTED_FILE",
        "source.cad",
        "PLANTED_SOURCE_NAME",
        "PLANTED_OBJECT_NAME",
        "PLANTED_LAYER",
        "PLANTED_MEMBER_PATH",
        "member.cad",
    ] {
        assert!(!json.contains(secret), "{secret}");
    }
    let parsed: Bundle = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.context, options.context);
}

#[test]
fn repro_native_feature_closure_keeps_numeric_joins_and_type_rows() {
    let mut document = CadIr::empty();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let ctx = cadmpeg_core::decode::DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
    let native = document.native.namespace_mut("synthetic");
    for (arena, records) in [
        (
            "scopes",
            serde_json::json!([{"id":"synthetic:scope:scope#1", "references": [2], "class": 17}]),
        ),
        (
            "operands",
            serde_json::json!([{"id":"synthetic:scope:operand#2", "members": [3]}]),
        ),
        (
            "members",
            serde_json::json!([{"id":"synthetic:scope:member#3", "recipe": 4}]),
        ),
        (
            "recipes",
            serde_json::json!([{"id":"synthetic:scope:recipe#4", "parameters": [5], "history": 6}]),
        ),
        (
            "parameters",
            serde_json::json!([{"id":"synthetic:scope:parameter#5", "value": 9.0}]),
        ),
        (
            "states",
            serde_json::json!([{"id":"synthetic:scope:state#6", "state": 10}]),
        ),
        (
            "types",
            serde_json::json!([{"id":"synthetic:scope:type#17", "class": 17}]),
        ),
    ] {
        native
            .set_arena(&ctx, arena, records.as_array().unwrap())
            .unwrap();
    }
    document.model.features.push(crate::features::Feature {
        product_definition: None,
        id: crate::features::FeatureId::mint("synthetic:model:feature#1").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: [(
            cadmpeg_core::text::NonBlankString::new("PLANTED_PROPERTY_PATH/member.cad").unwrap(),
            "PLANTED_PROPERTY_VALUE".into(),
        )]
        .into(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: crate::features::FeatureEvaluation::from_definition(
            crate::features::FeatureDefinition::Operation(
                crate::features::FeatureOperation::Native {
                    kind: "scope".into(),
                    parameters: Default::default(),
                },
            ),
        ),
        native_ref: Some("synthetic:scope:scope#1".into()),
    });
    let finding = crate::report::check::Finding {
        check: Check::NativeLinks,
        severity: crate::report::Severity::Error,
        entity: Some("synthetic:scope:scope#1".into()),
        related: Vec::new(),
        message: "native feature is inconsistent".into(),
    };
    let bundle = cut(&document, &finding, &CutOptions::default()).unwrap();
    assert!(!serde_json::to_string(&bundle).unwrap().contains("PLANTED"));
    let native = bundle.document.native.namespace("synthetic").unwrap();
    for arena in [
        "scopes",
        "operands",
        "members",
        "recipes",
        "parameters",
        "states",
        "types",
    ] {
        assert_eq!(native.arenas()[arena].len(), 1);
    }
    let mut model_finding = finding.clone();
    model_finding.entity = Some("synthetic:model:feature#1".into());
    let model_bundle = cut(&document, &model_finding, &CutOptions::default()).unwrap();
    assert_eq!(
        model_bundle
            .document
            .native
            .namespace("synthetic")
            .unwrap()
            .arenas()["types"]
            .len(),
        1
    );
    assert_eq!(model_bundle.document.model.features.len(), 1);
    let parsed: CadIr = serde_json::from_slice(&serde_json::to_vec(&bundle).unwrap()).unwrap();
    assert!(parsed.fragment);
    assert_eq!(parsed.repro.unwrap().context, bundle.context);
}

#[test]
fn repro_cut_caps_rows_and_serialized_bytes() {
    let (document, finding) = failing_cube();
    assert!(cut(
        &document,
        &finding,
        &CutOptions {
            max_rows: 1,
            ..CutOptions::default()
        }
    )
    .is_err());
    assert!(cut(
        &document,
        &finding,
        &CutOptions {
            max_serialized_bytes: 1,
            ..CutOptions::default()
        }
    )
    .is_err());
}

#[test]
fn repro_edge_closure_keeps_pcurves_and_use_curve_carriers() {
    use crate::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata};
    use crate::topology::{CoedgeUseCurve, ParameterInterval, PcurveUse};
    let (mut document, finding) = failing_cube();
    let mut carrier = document.model.curves[0].clone();
    carrier.id = crate::ids::CurveId::mint("synthetic:use:curve#1").unwrap();
    let curve = carrier.id.clone();
    document.model.curves.push(carrier);
    let pcurve = crate::ids::PcurveId::mint("synthetic:use:pcurve#1").unwrap();
    document.model.pcurves.push(Pcurve {
        id: pcurve.clone(),
        geometry: PcurveGeometry::Line(
            LinePcurve::try_new(
                crate::math::Point2::new(0.0, 0.0),
                crate::math::Point2::new(1.0, 0.0),
            )
            .unwrap(),
        ),
        metadata: PcurveMetadata::default(),
    });
    let coedge = document
        .model
        .coedges
        .iter_mut()
        .find(|coedge| Some(coedge.edge.as_str()) == finding.entity.as_deref())
        .unwrap();
    coedge.use_curve = Some(CoedgeUseCurve {
        curve,
        parameter_range: ParameterInterval::new([0.0, 10.0]).unwrap(),
    });
    coedge.pcurves.push(PcurveUse {
        pcurve,
        isoparametric: None,
        parameter_range: None,
    });
    document.finalize();
    let bundle = cut(&document, &finding, &CutOptions::default()).unwrap();
    assert_eq!(bundle.document.model.pcurves.len(), 1);
    assert!(bundle.document.model.coedges.iter().any(|coedge| coedge
        .use_curve
        .as_ref()
        .is_some_and(|use_curve| {
            bundle
                .document
                .model
                .curves
                .iter()
                .any(|curve| curve.id == use_curve.curve)
        })));
    assert!(bundle
        .document
        .model
        .coedges
        .iter()
        .flat_map(|coedge| &coedge.pcurves)
        .all(|use_| bundle
            .document
            .model
            .pcurves
            .iter()
            .any(|curve| curve.id == use_.pcurve)));
}

#[test]
fn repro_face_closure_keeps_the_complete_shell() {
    let document = crate::examples::unit_cube().unwrap();
    let finding = crate::report::check::Finding {
        check: Check::Orientation,
        severity: crate::report::Severity::Warning,
        entity: Some(document.model.faces[0].id.as_str().into()),
        related: Vec::new(),
        message: "face winding is inconsistent".into(),
    };
    let bundle = cut(&document, &finding, &CutOptions::default()).unwrap();
    assert_eq!(bundle.document.model.faces.len(), 6);
    assert_eq!(bundle.document.model.coedges.len(), 24);
    assert_eq!(bundle.document.model.shells.len(), 1);
}

#[test]
fn repro_display_name_equal_to_a_geometry_tag_is_redacted() {
    let (mut document, finding) = failing_cube();
    document.model.bodies[0].name = Some("line".into());
    let bundle = cut(&document, &finding, &CutOptions::default()).unwrap();
    assert_ne!(
        bundle.document.model.bodies[0].name.as_deref(),
        Some("line")
    );
    assert!(crate::validate_neutral(&bundle.document, Vec::new())
        .unwrap()
        .findings
        .iter()
        .any(|finding| bundle.signature.reproduces(finding)));
}

#[test]
fn repro_classified_source_keeps_codec_and_strips_declarations() {
    use cadmpeg_core::dialect::{DialectLayers, DialectMatch};
    let (mut document, finding) = failing_cube();
    let matched = DialectMatch::admitted(cadmpeg_core::dialect_id!("f3d:manifest-3-2-0-0"))
        .with_declared_entry(
            cadmpeg_core::nonblank_literal!("carrier"),
            "PLANTED_MEMBER/source.cad",
        );
    document.source = Some(crate::document::SourceMeta::classified(
        DialectLayers::of(matched),
        Default::default(),
    ));
    let bundle = cut(&document, &finding, &CutOptions::default()).unwrap();
    assert_eq!(bundle.document.source.as_ref().unwrap().format(), "f3d");
    assert_eq!(bundle.signature.codec, "f3d");
    assert!(!serde_json::to_string(&bundle)
        .unwrap()
        .contains("PLANTED_MEMBER"));
}
