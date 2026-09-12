// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

use cadmpeg_core::dialect::{DialectId, DialectLayers};

use crate::SourceProvenance;

use super::*;

#[test]
fn loss_code_serializes_as_namespaced_object() {
    let note = LossNote::new(
        LossKind::shared(LossTaxonomy::TopologyNotTransferred),
        "topology graph not transferred",
    )
    .with_severity(Severity::Blocking);
    let value: serde_json::Value = serde_json::to_value(&note).expect("required invariant");
    assert_eq!(value["code"]["scope"], "shared");
    assert_eq!(value["code"]["kind"], "topology_not_transferred");
    assert!(value["code"].get("namespace").is_none());
    assert!(value["code"].get("code").is_none());
    assert!(value["code"].get("strict_floor").is_none());
    assert_eq!(
        note.code.to_string(),
        format!("{SHARED_LOSS_NAMESPACE}/topology_not_transferred")
    );
}

#[test]
fn a_shared_loss_kind_carries_no_code_and_no_strict_floor() {
    let shared = LossKind::shared(LossTaxonomy::TopologyNotTransferred);
    let wire = serde_json::to_value(&shared).expect("serializes");
    assert_eq!(
        wire,
        serde_json::json!({"scope": "shared", "kind": "topology_not_transferred"})
    );
    assert_eq!(
        serde_json::from_value::<LossKind>(wire).expect("round trip"),
        shared
    );

    let error = serde_json::from_value::<LossKind>(serde_json::json!({
        "scope": "shared",
        "kind": "topology_not_transferred",
        "code": "geometry_not_transferred"
    }))
    .unwrap_err()
    .to_string();
    assert!(error.contains("code"), "{error}");

    let error = serde_json::from_value::<LossKind>(serde_json::json!({
        "scope": "shared",
        "kind": "topology_not_transferred",
        "strict_floor": "error"
    }))
    .unwrap_err()
    .to_string();
    assert!(error.contains("strict_floor"), "{error}");

    let namespaced = LossKind::namespaced(
        const {
            match LossNamespace::new("rhino") {
                Ok(namespace) => namespace,
                Err(_) => panic!("reserved codec namespace"),
            }
        },
        "brep.trim-pcurve-dropped",
        LossTaxonomy::TopologyNotTransferred,
    );
    let wire = serde_json::to_value(&namespaced).expect("serializes");
    assert_eq!(
        wire,
        serde_json::json!({
            "scope": "namespaced",
            "namespace": "rhino",
            "code": "brep.trim-pcurve-dropped",
            "kind": "topology_not_transferred",
            "strict_floor": "warning"
        })
    );
    assert_eq!(
        serde_json::from_value::<LossKind>(wire).expect("round trip"),
        namespaced
    );

    let error = serde_json::from_value::<LossKind>(serde_json::json!({
        "scope": "namespaced",
        "namespace": "shared",
        "code": "topology_not_transferred",
        "kind": "topology_not_transferred"
    }))
    .unwrap_err()
    .to_string();
    assert!(error.contains("shared"), "{error}");
}

#[test]
fn loss_taxonomy_static_names_match_the_serde_wire_names() {
    for taxonomy in [
        LossTaxonomy::MissingGeometryStream,
        LossTaxonomy::TopologyNotTransferred,
        LossTaxonomy::SourceTopologyInvalid,
        LossTaxonomy::GeometryNotTransferred,
        LossTaxonomy::ReferenceGraphNotClosed,
        LossTaxonomy::TopologyGaugeSubstituted,
        LossTaxonomy::CarrierAxisInferred,
        LossTaxonomy::CarrierSummary,
        LossTaxonomy::MaterialNotTransferred,
        LossTaxonomy::MetadataNotTransferred,
        LossTaxonomy::AttributesNotTransferred,
        LossTaxonomy::FeatureHistoryRetained,
        LossTaxonomy::AssemblyComponentsExternal,
        LossTaxonomy::AssemblyPlacementsNotTransferred,
        LossTaxonomy::RecordNotTyped,
        LossTaxonomy::DecodeDiagnostic,
        LossTaxonomy::IntegrityFailure,
        LossTaxonomy::NoncanonicalSourceSyntax,
        LossTaxonomy::SourceDialectUnverified,
        LossTaxonomy::SourceDialectDisplaced,
        LossTaxonomy::MeshVertexPrecision,
        LossTaxonomy::ObjectRecordsUntransferred,
        LossTaxonomy::UnsupportedObjectFamily,
        LossTaxonomy::AssetNotTransferred,
        LossTaxonomy::NoExportableSolids,
        LossTaxonomy::HiddenBodyOmitted,
        LossTaxonomy::BodyTransformNotApplied,
        LossTaxonomy::AnalyticSurfaceNormalized,
        LossTaxonomy::EllipticalConeReduced,
        LossTaxonomy::CurvelessEdgeOmitted,
        LossTaxonomy::UnknownSurfaceFaceOmitted,
        LossTaxonomy::PcurveOmitted,
        LossTaxonomy::SubdOmitted,
        LossTaxonomy::TessellationOmitted,
        LossTaxonomy::PmiOmitted,
        LossTaxonomy::SourceAssociationOmitted,
        LossTaxonomy::PassthroughRecordOmitted,
        LossTaxonomy::ProceduralReduced,
        LossTaxonomy::ParametricRecordOmitted,
        LossTaxonomy::AppearanceReduced,
        LossTaxonomy::PreservedSourceUnavailable,
    ] {
        assert_eq!(
            serde_json::to_value(taxonomy).unwrap(),
            serde_json::Value::String(taxonomy.as_str().to_owned())
        );
    }
}

#[test]
fn loss_kind_strict_consequence_depends_on_severity() {
    assert_eq!(
        LossNote::new(
            LossKind::shared(LossTaxonomy::TopologyNotTransferred),
            "missing topology"
        )
        .strict_consequence(),
        StrictConsequence::Reject
    );
    assert_eq!(
        LossNote::new(
            LossKind::shared(LossTaxonomy::TopologyNotTransferred),
            "diagnostic"
        )
        .with_severity(Severity::Info)
        .strict_consequence(),
        StrictConsequence::Tolerate
    );
    assert_eq!(
        LossNote::new(
            LossKind::shared(LossTaxonomy::PassthroughRecordOmitted),
            "retained source"
        )
        .strict_consequence(),
        StrictConsequence::Tolerate
    );
}

#[test]
fn assembly_losses_belong_to_the_product_domain() {
    assert_eq!(
        LossKind::shared(LossTaxonomy::AssemblyComponentsExternal).category(),
        LossCategory::Product
    );
    assert_eq!(
        LossKind::shared(LossTaxonomy::AssemblyPlacementsNotTransferred).category(),
        LossCategory::Product
    );
    assert_eq!(
        LossKind::shared(LossTaxonomy::AssemblyPlacementsNotTransferred).to_string(),
        format!("{SHARED_LOSS_NAMESPACE}/assembly_placements_not_transferred")
    );
}

#[test]
fn noncanonical_source_syntax_is_a_strict_rejectable_warning() {
    let kind = LossKind::shared(LossTaxonomy::NoncanonicalSourceSyntax);
    assert_eq!(
        kind.to_string(),
        format!("{SHARED_LOSS_NAMESPACE}/noncanonical_source_syntax")
    );
    assert_eq!(kind.category(), LossCategory::Other);
    assert_eq!(kind.default_severity(), Severity::Warning);
    assert_eq!(kind.strict_floor(), Some(Severity::Warning));
    assert_eq!(
        LossNote::new(kind, "source order is noncanonical").strict_consequence(),
        StrictConsequence::Reject
    );
}

#[test]
fn integrity_failure_is_a_strict_rejectable_error() {
    let kind = LossKind::shared(LossTaxonomy::IntegrityFailure);
    assert_eq!(
        kind.to_string(),
        format!("{SHARED_LOSS_NAMESPACE}/integrity_failure")
    );
    assert_eq!(kind.category(), LossCategory::Other);
    assert_eq!(kind.default_severity(), Severity::Error);
    assert_eq!(kind.strict_floor(), Some(Severity::Warning));
    assert_eq!(
        LossNote::new(kind, "stored checksum differs").strict_consequence(),
        StrictConsequence::Reject
    );
}

#[test]
fn namespaced_local_code_pins_strict_floor_independently_of_taxonomy() {
    let kind: LossKind = NamespacedLossKind::new(
        LossNamespace::new("sldprt").unwrap(),
        "geometry.pcurve-ambiguous",
        LossTaxonomy::PcurveOmitted,
    )
    .with_strict_floor(None)
    .into();
    let roundtrip: LossKind = serde_json::from_value(serde_json::to_value(&kind).unwrap()).unwrap();
    assert_eq!(roundtrip.namespace(), "sldprt");
    assert_eq!(roundtrip.local_code(), "geometry.pcurve-ambiguous");
    assert_eq!(roundtrip.taxonomy(), LossTaxonomy::PcurveOmitted);
    assert_eq!(roundtrip.strict_floor(), None);
    assert_eq!(
        LossNote::new(roundtrip, "ambiguous").strict_consequence(),
        StrictConsequence::Tolerate
    );
}

#[test]
fn loss_provenance_root_alias_constructs_and_serializes() {
    let note = LossNote::new(
        LossKind::shared(LossTaxonomy::GeometryNotTransferred),
        "geometry was retained as metadata",
    )
    .with_severity(Severity::Warning)
    .with_provenance(
        SourceProvenance::root("rhino", 42)
            .with_tag("OBJECT_RECORD/class=00000000-0000-0000-0000-000000000000/type=0x00000020"),
    );
    let json = serde_json::to_value(&note).unwrap();
    assert_eq!(json["provenance"]["format"], "rhino");
    assert!(json["provenance"].get("stream").is_none());
    assert_eq!(json["provenance"]["offset"], 42);
    assert_eq!(
        json["provenance"]["tag"],
        "OBJECT_RECORD/class=00000000-0000-0000-0000-000000000000/type=0x00000020"
    );
}

/// The dialect fields are part of the wire format: a report that named nothing
/// says so with `null`, rather than by omitting the key.
/// Reports written before the fields existed still read back.
#[test]
fn unclassified_reports_serialize_empty_dialect_keys() {
    let decode = DecodeReport::unclassified(
        "rhino",
        DecodeTransfer::full(true),
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
        TransferLedger::default(),
    );
    let rendered = serde_json::to_string(&decode).unwrap();
    assert!(
        rendered
            .contains("\"identity\":{\"classification\":\"unclassified\",\"format\":\"rhino\"}"),
        "{rendered}"
    );
    assert_eq!(
        serde_json::from_str::<DecodeReport>(&rendered).unwrap(),
        decode
    );

    let export = ExportReport::cadir(
        EntityCensus {
            basis: CensusBasis::TargetRecords,
            counts: BTreeMap::new(),
        },
        crate::codec::write::WritePath::Synthesized {
            consumption: crate::codec::write::Consumption::NotConsumed,
        },
        false,
        Vec::new(),
        Vec::new(),
    );
    let rendered = serde_json::to_string(&export).unwrap();
    assert!(rendered.contains("\"payload\":\"cadir\""), "{rendered}");
    assert!(!rendered.contains("\"target\""), "{rendered}");
    assert_eq!(
        serde_json::from_str::<ExportReport>(&rendered).unwrap(),
        export
    );
}

#[test]
fn a_native_export_report_states_its_payload_and_names_its_target() {
    let report = ExportReport::native(
        DialectId::pinned("step:ap242-e3"),
        EntityCensus {
            basis: CensusBasis::TargetRecords,
            counts: BTreeMap::new(),
        },
        crate::codec::write::WritePath::Synthesized {
            consumption: crate::codec::write::Consumption::NotConsumed,
        },
        false,
        Vec::new(),
        Vec::new(),
    );

    assert_eq!(report.format(), "step");
    assert_eq!(
        report.target().map(DialectId::as_str),
        Some("step:ap242-e3")
    );
    let rendered = serde_json::to_value(&report).unwrap();
    assert_eq!(rendered["identity"]["payload"], "native");
    assert_eq!(rendered["identity"]["target"], "step:ap242-e3");
    assert!(rendered.get("format").is_none());
    assert_eq!(
        serde_json::from_value::<ExportReport>(rendered).unwrap(),
        report
    );
}

#[test]
fn an_export_payload_carries_only_the_target_key_its_own_arm_owns() {
    let native = serde_json::json!({
        "identity": {"payload": "native", "target": "step:ap242-e3"},
        "census": { "basis": "target_records", "counts": {} },
        "fidelity": { "status": "not_provided" },
        "write_path": "synthesized",
        "losses": [],
        "notes": [],
    });
    serde_json::from_value::<ExportReport>(native.clone()).expect("a native export report");

    let mut without_target = native.clone();
    without_target["identity"]
        .as_object_mut()
        .expect("map")
        .remove("target");
    let error = serde_json::from_value::<ExportReport>(without_target)
        .expect_err("a native export report requires a target");
    assert!(error.to_string().contains("target"), "{error}");

    let mut cadir_with_target = native;
    cadir_with_target["identity"]["payload"] = serde_json::json!("cadir");
    cadir_with_target["census"]["basis"] = serde_json::json!("ir_arenas");
    let error = serde_json::from_value::<ExportReport>(cadir_with_target)
        .expect_err("CADIR has no native dialect target");
    assert!(error.to_string().contains("target"), "{error}");
}

#[test]
fn classified_report_wire_requires_its_primary_format() {
    let report = DecodeReport::classified(
        DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            DialectId::pinned("rhino:archive-80"),
        )),
        DecodeTransfer::full(true),
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
        TransferLedger::default(),
    );
    let golden = serde_json::to_string(&report).unwrap();
    assert_eq!(
        golden,
        r#"{"identity":{"classification":"classified","dialects":{"primary":{"format":"rhino","dialect":"rhino:archive-80","admission":"admitted"},"extra":[]}},"transfer":{"transfer":"full","geometry_transferred":true},"losses":[],"notes":[]}"#
    );
    assert_eq!(
        serde_json::from_str::<DecodeReport>(&golden).unwrap(),
        report
    );

    let contradictory = golden.replacen(
        "\"transfer\":\"full\"",
        "\"transfer\":\"container_only\"",
        1,
    );
    let error = serde_json::from_str::<DecodeReport>(&contradictory)
        .expect_err("a container-only report has no geometry outcome to state");
    assert!(
        error.to_string().contains("geometry_transferred"),
        "{error}"
    );

    let restated = golden.replacen(
        "\"classification\":\"classified\"",
        "\"classification\":\"classified\",\"format\":\"step\"",
        1,
    );
    let error = serde_json::from_str::<DecodeReport>(&restated)
        .expect_err("a classified identity carries no second format");
    assert!(error.to_string().contains("format"), "{error}");
}

#[test]
fn container_only_report_wire_preserves_the_coherent_transfer_state() {
    let report = DecodeReport::unclassified(
        "test",
        DecodeTransfer::ContainerOnly {},
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
        TransferLedger::default(),
    );

    let rendered = serde_json::to_string(&report).unwrap();
    assert!(
        rendered.contains("\"transfer\":\"container_only\""),
        "{rendered}"
    );
    assert!(!rendered.contains("geometry_transferred"), "{rendered}");
    assert_eq!(
        serde_json::from_str::<DecodeReport>(&rendered).unwrap(),
        report
    );
}

#[test]
fn namespaced_loss_rejects_reserved_namespace() {
    assert_eq!(LossNamespace::new("shared"), Err(LossNamespaceError));
    let namespace = String::from("shared");
    assert_eq!(LossNamespace::new(&namespace), Err(LossNamespaceError));
    assert!(serde_json::from_value::<LossKind>(serde_json::json!({
        "namespace": "shared", "code": "wrong", "kind": "pcurve_omitted"
    }))
    .is_err());
}

#[test]
fn a_decode_transfer_states_its_scope_and_carries_only_its_own_keys() {
    let base = serde_json::json!({
        "identity": {"classification": "unclassified", "format": "rhino"},
        "transfer": {"transfer": "full", "geometry_transferred": true},
        "losses": [],
        "notes": [],
    });
    let report = serde_json::from_value::<DecodeReport>(base.clone()).expect("a full decode");
    assert_eq!(report.transfer(), DecodeTransfer::full(true));
    assert_eq!(
        serde_json::to_value(&report).unwrap()["transfer"]["transfer"],
        "full"
    );

    let mut container_only = base.clone();
    container_only["transfer"]["transfer"] = serde_json::json!("container_only");
    let error = serde_json::from_value::<DecodeReport>(container_only.clone())
        .expect_err("a container-only report has no geometry outcome");
    assert!(
        error.to_string().contains("geometry_transferred"),
        "{error}"
    );

    container_only["transfer"]
        .as_object_mut()
        .expect("map")
        .remove("geometry_transferred");
    let report = serde_json::from_value::<DecodeReport>(container_only).expect("container only");
    assert_eq!(report.transfer(), DecodeTransfer::ContainerOnly {});
    let wire = serde_json::to_value(&report).unwrap();
    assert_eq!(wire["transfer"]["transfer"], "container_only");
    assert!(wire["transfer"].get("geometry_transferred").is_none());

    let mut without_outcome = base;
    without_outcome["transfer"]
        .as_object_mut()
        .expect("map")
        .remove("geometry_transferred");
    let error = serde_json::from_value::<DecodeReport>(without_outcome)
        .expect_err("a full decode states its geometry outcome");
    assert!(
        error.to_string().contains("geometry_transferred"),
        "{error}"
    );
}
