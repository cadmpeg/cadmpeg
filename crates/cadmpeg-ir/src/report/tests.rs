// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;

#[test]
fn loss_code_serializes_as_namespaced_object() {
    let note = LossNote::new(
        LossKind::shared(LossTaxonomy::TopologyNotTransferred),
        "topology graph not transferred",
    )
    .with_severity(Severity::Blocking);
    let value: serde_json::Value = serde_json::to_value(&note).expect("required invariant");
    assert_eq!(value["code"]["namespace"], SHARED_LOSS_NAMESPACE);
    assert_eq!(value["code"]["code"], "topology_not_transferred");
    assert_eq!(value["code"]["kind"], "topology_not_transferred");
    assert!(value["code"].get("strict_floor").is_none());
    assert_eq!(
        note.code.as_str(),
        format!("{SHARED_LOSS_NAMESPACE}/topology_not_transferred")
    );
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
        LossKind::shared(LossTaxonomy::AssemblyPlacementsNotTransferred).as_str(),
        format!("{SHARED_LOSS_NAMESPACE}/assembly_placements_not_transferred")
    );
}

#[test]
fn noncanonical_source_syntax_is_a_strict_rejectable_warning() {
    let kind = LossKind::shared(LossTaxonomy::NoncanonicalSourceSyntax);
    assert_eq!(
        kind.as_str(),
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
        kind.as_str(),
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
    let kind = LossKind::namespaced(
        "sldprt",
        "geometry.pcurve-ambiguous",
        LossTaxonomy::PcurveOmitted,
    )
    .with_strict_floor(None);
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
    .with_provenance(SourceProvenance {
        format: "rhino".into(),
        stream: String::new(),
        offset: 42,
        tag: Some(
            "OBJECT_RECORD/class=00000000-0000-0000-0000-000000000000/type=0x00000020".into(),
        ),
    });
    let json = serde_json::to_value(&note).unwrap();
    assert_eq!(json["provenance"]["format"], "rhino");
    assert_eq!(json["provenance"]["stream"], "");
    assert_eq!(json["provenance"]["offset"], 42);
    assert_eq!(
        json["provenance"]["tag"],
        "OBJECT_RECORD/class=00000000-0000-0000-0000-000000000000/type=0x00000020"
    );
}

/// The dialect fields are part of the wire format: a report that named nothing
/// says so with an empty list and a `null`, rather than by omitting the key.
/// Reports written before the fields existed still read back.
#[test]
fn unclassified_reports_serialize_empty_dialect_keys() {
    let decode = DecodeReport {
        format: "rhino".into(),
        container_only: false,
        geometry_transferred: true,
        coverage: BTreeMap::new(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: TransferLedger::default(),
        dialects: Vec::new(),
    };
    let rendered = serde_json::to_string(&decode).unwrap();
    assert!(rendered.contains("\"dialects\":[]"), "{rendered}");
    assert_eq!(
        serde_json::from_str::<DecodeReport>(&rendered).unwrap(),
        decode
    );

    // A report persisted before the field existed omits the key entirely.
    let legacy = rendered.replace(",\"dialects\":[]", "");
    assert!(!legacy.contains("dialects"), "{legacy}");
    assert_eq!(
        serde_json::from_str::<DecodeReport>(&legacy).unwrap(),
        decode
    );

    let export = ExportReport::cadir(
        "cadir".into(),
        EntityCensus {
            basis: CensusBasis::TargetRecords,
            counts: BTreeMap::new(),
        },
        FidelityResolution::NotProvided,
        WritePath::Synthesized,
        Vec::new(),
        Vec::new(),
    );
    let rendered = serde_json::to_string(&export).unwrap();
    assert!(rendered.contains("\"target\":null"), "{rendered}");
    assert_eq!(
        serde_json::from_str::<ExportReport>(&rendered).unwrap(),
        export
    );

    // An export report persisted before the field existed omits the key.
    let legacy = rendered.replace(",\"target\":null", "");
    assert!(!legacy.contains("\"target\":"), "{legacy}");
    assert_eq!(
        serde_json::from_str::<ExportReport>(&legacy).unwrap(),
        export
    );
}
