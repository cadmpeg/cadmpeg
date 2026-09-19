// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::report::{
    loss::{
        LossCategory, LossKind, LossNamespace, LossNamespaceError, LossNote, LossTaxonomy,
        NamespacedLossKind, StrictConsequence, SHARED_LOSS_NAMESPACE,
    },
    Severity,
};

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
fn namespaced_loss_rejects_reserved_namespace() {
    assert_eq!(LossNamespace::new("shared"), Err(LossNamespaceError));
    let namespace = String::from("shared");
    assert_eq!(LossNamespace::new(&namespace), Err(LossNamespaceError));
    assert!(serde_json::from_value::<LossKind>(serde_json::json!({
        "namespace": "shared", "code": "wrong", "kind": "pcurve_omitted"
    }))
    .is_err());
}
