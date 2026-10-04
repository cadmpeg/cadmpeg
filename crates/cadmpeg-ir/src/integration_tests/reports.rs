// SPDX-License-Identifier: Apache-2.0
use crate::report::{
    decode::{DecodeReport, DecodeTransfer, TransferLedger},
    export::{CensusBasis, EntityCensus, ExportReport},
    loss::{LossKind, LossNote, LossTaxonomy},
    Severity,
};
use crate::SourceProvenance;
use std::collections::BTreeMap;

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
        crate::report::export::WritePath::Synthesized {
            fidelity: crate::report::export::SynthesisFidelity::NotProvided {},
        },
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
