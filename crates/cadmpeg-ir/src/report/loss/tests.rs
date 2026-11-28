// SPDX-License-Identifier: Apache-2.0
use crate::report::{
    loss::{
        LossCategory, LossKind, LossNamespace, LossNamespaceError, LossNote, LossTaxonomy,
        NamespacedLossKind, SHARED_LOSS_NAMESPACE,
    },
    Severity,
};
use cadmpeg_core::serde_erase::json;

#[test]
fn owned_namespaced_loss_kind_keeps_taxonomy_and_rejects_reserved_namespace() {
    let taxonomy = LossTaxonomy::TopologyNotTransferred;
    let owned = NamespacedLossKind::new_owned(
        "catia".to_owned(),
        "topology.unresolved".to_owned(),
        taxonomy,
    )
    .expect("codec namespace is admitted");
    let borrowed = NamespacedLossKind::new(
        LossNamespace::new("catia").expect("codec namespace"),
        "topology.unresolved",
        taxonomy,
    );
    assert_eq!(owned, borrowed);
    assert!(NamespacedLossKind::new_owned(
        "shared".to_owned(),
        "topology.unresolved".to_owned(),
        taxonomy,
    )
    .is_err());
}

#[test]
fn loss_code_serializes_as_namespaced_object() {
    let note = LossNote::new(
        LossKind::shared(LossTaxonomy::TopologyNotTransferred),
        "topology graph not transferred",
    )
    .with_severity(Severity::Blocking);
    let value: serde_json::Value = json::to_value(&note).expect("required invariant");
    assert_eq!(value["code"]["kind"], "shared");
    assert_eq!(value["code"]["value"]["kind"], "topology_not_transferred");
    assert!(value["code"]["value"].get("namespace").is_none());
    assert!(value["code"]["value"].get("code").is_none());
    assert!(value["code"]["value"].get("strict_floor").is_none());
    assert_eq!(
        note.code.to_string(),
        format!("{SHARED_LOSS_NAMESPACE}/topology_not_transferred")
    );
}

#[test]
fn a_shared_loss_kind_carries_no_code_and_no_strict_floor() {
    let shared = LossKind::shared(LossTaxonomy::TopologyNotTransferred);
    let wire = json::to_value(&shared).expect("serializes");
    assert_eq!(
        wire,
        cadmpeg_core::json!({"kind": "shared", "value": {"kind": "topology_not_transferred"}})
    );
    assert_eq!(
        json::from_value::<LossKind>(wire).expect("round trip"),
        shared
    );

    let error = json::from_value::<LossKind>(cadmpeg_core::json!({"kind": "shared", "value": {
        "kind": "topology_not_transferred",
        "code": "geometry_not_transferred"
    }}))
    .unwrap_err()
    .to_string();
    assert!(error.contains("code"), "{error}");

    let error = json::from_value::<LossKind>(cadmpeg_core::json!({"kind": "shared", "value": {
        "kind": "topology_not_transferred",
        "strict_floor": "error"
    }}))
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
    let wire = json::to_value(&namespaced).expect("serializes");
    assert_eq!(
        wire,
        cadmpeg_core::json!({"kind": "namespaced", "value": {
            "namespace": "rhino",
            "code": "brep.trim-pcurve-dropped",
            "kind": "topology_not_transferred"
        }})
    );
    assert_eq!(
        json::from_value::<LossKind>(wire).expect("round trip"),
        namespaced
    );

    let error = json::from_value::<LossKind>(cadmpeg_core::json!({"kind": "namespaced", "value": {
        "namespace": "shared",
        "code": "topology_not_transferred",
        "kind": "topology_not_transferred"
    }}))
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
        LossTaxonomy::PointCloudOmitted,
        LossTaxonomy::PmiOmitted,
        LossTaxonomy::SourceAssociationOmitted,
        LossTaxonomy::PassthroughRecordOmitted,
        LossTaxonomy::ProceduralReduced,
        LossTaxonomy::ParametricRecordOmitted,
        LossTaxonomy::AppearanceReduced,
        LossTaxonomy::PreservedSourceUnavailable,
    ] {
        assert_eq!(
            json::to_value(taxonomy).unwrap(),
            serde_json::Value::String(taxonomy.as_str().to_owned())
        );
    }
}

/// §3.2 states no strict floor for a shared taxonomy code.
#[test]
fn a_shared_taxonomy_code_pins_no_strict_floor() {
    for taxonomy in [
        LossTaxonomy::TopologyNotTransferred,
        LossTaxonomy::NoncanonicalSourceSyntax,
        LossTaxonomy::IntegrityFailure,
        LossTaxonomy::PassthroughRecordOmitted,
    ] {
        assert_eq!(
            LossKind::shared(taxonomy).strict_floor(),
            None,
            "{taxonomy}"
        );
    }
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
fn noncanonical_source_syntax_is_an_other_category_warning() {
    let kind = LossKind::shared(LossTaxonomy::NoncanonicalSourceSyntax);
    assert_eq!(
        kind.to_string(),
        format!("{SHARED_LOSS_NAMESPACE}/noncanonical_source_syntax")
    );
    assert_eq!(kind.category(), LossCategory::Other);
    assert_eq!(kind.default_severity(), Severity::Warning);
}

#[test]
fn integrity_failure_is_an_other_category_error() {
    let kind = LossKind::shared(LossTaxonomy::IntegrityFailure);
    assert_eq!(
        kind.to_string(),
        format!("{SHARED_LOSS_NAMESPACE}/integrity_failure")
    );
    assert_eq!(kind.category(), LossCategory::Other);
    assert_eq!(kind.default_severity(), Severity::Error);
}

/// §3.2 states no strict floor for `target.source-dialect-displaced` or
/// `target.request-displaced`, whichever codec charges them.
#[test]
fn a_displaced_target_dialect_pins_no_strict_floor() {
    for code in [
        "target.source-dialect-displaced",
        "target.request-displaced",
    ] {
        let kind = NamespacedLossKind::new(
            LossNamespace::new("rhino").unwrap(),
            code,
            LossTaxonomy::SourceDialectDisplaced,
        );
        assert_eq!(LossKind::from(kind).strict_floor(), None, "{code}");
    }
}

/// A bare shared code takes the floor §3.2 states for it, whichever codec
/// charges it. Any other code pins the floor its owner states.
#[test]
fn a_bare_shared_code_takes_the_floor_section_3_2_states() {
    let kind = |code: &str| {
        LossKind::namespaced(
            LossNamespace::new("nx").unwrap(),
            code,
            LossTaxonomy::TopologyNotTransferred,
        )
    };
    assert_eq!(kind("topology.face-boundary-unstated").strict_floor(), None);
    assert_eq!(
        kind("record.unreadable").strict_floor(),
        Some(Severity::Error)
    );
    assert_eq!(kind("record.undecoded").strict_floor(), None);
    assert_eq!(kind("topology.face-omitted").strict_floor(), None);
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
    let roundtrip: LossKind = json::from_value(json::to_value(&kind).unwrap()).unwrap();
    assert_eq!(roundtrip.namespace(), "sldprt");
    assert_eq!(roundtrip.local_code(), "geometry.pcurve-ambiguous");
    assert_eq!(roundtrip.taxonomy(), LossTaxonomy::PcurveOmitted);
    assert_eq!(roundtrip.strict_floor(), None);
}

#[test]
fn namespaced_loss_rejects_reserved_namespace() {
    assert_eq!(LossNamespace::new("shared"), Err(LossNamespaceError));
    let namespace = String::from("shared");
    assert_eq!(LossNamespace::new(&namespace), Err(LossNamespaceError));
    assert!(
        json::from_value::<LossKind>(cadmpeg_core::json!({"kind": "namespaced", "value": {
            "namespace": "shared", "code": "wrong", "kind": "pcurve_omitted"
        }}))
        .is_err()
    );
}

#[test]
fn loss_clone_preserves_provenance() {
    let original = LossNote::new(
        LossKind::namespaced(
            LossNamespace::new("rhino").expect("codec namespace"),
            "geometry-dropped",
            LossTaxonomy::GeometryNotTransferred,
        ),
        "source geometry omitted",
    )
    .with_provenance(crate::SourceProvenance::root("rhino", 7).with_tag("object"));
    let copied = original.clone();
    assert_eq!(copied, original);
}
