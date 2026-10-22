// SPDX-License-Identifier: Apache-2.0
//! Shared export factories fix the code, category and severity for each loss.

use super::ExportLossCode;
use crate::report::{
    loss::{LossNote, LossTaxonomy},
    Severity,
};

#[test]
fn shared_export_codes_have_fixed_namespace_and_severity() {
    for (factory, code, severity, taxonomy) in [
        (
            ExportLossCode::SourceUndecodedOmitted,
            "source.undecoded-omitted",
            Severity::Warning,
            LossTaxonomy::RecordNotTyped,
        ),
        (
            ExportLossCode::RecordResidualUnavailable,
            "record.residual-unavailable",
            Severity::Warning,
            LossTaxonomy::PreservedSourceUnavailable,
        ),
        (
            ExportLossCode::ValueQuantized,
            "value.quantized",
            Severity::Info,
            LossTaxonomy::ValueQuantized,
        ),
        (
            ExportLossCode::OutputEmpty,
            "output.empty",
            Severity::Warning,
            LossTaxonomy::NoExportableSolids,
        ),
        (
            ExportLossCode::NativeRecordsOmitted,
            "native.records-omitted",
            Severity::Info,
            LossTaxonomy::RecordNotTyped,
        ),
        (
            ExportLossCode::EvaluationStale,
            "evaluation.stale",
            Severity::Warning,
            LossTaxonomy::FeatureHistoryRetained,
        ),
    ] {
        let note = factory.note("record test:native:record#one");
        assert_eq!(note.code.namespace(), "export");
        assert_eq!(note.code.local_code(), code);
        assert_eq!(note.severity, severity);
        assert_eq!(note.code.taxonomy(), taxonomy);
        assert_eq!(note.message, "record test:native:record#one");
        let wire = serde_json::to_string(&note).expect("loss note serializes");
        assert_eq!(
            serde_json::from_str::<LossNote>(&wire).expect("loss note round trips"),
            note
        );
    }
}
