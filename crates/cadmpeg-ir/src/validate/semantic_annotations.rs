// SPDX-License-Identifier: Apache-2.0
//! Semantic annotation references and reference anchors (§8).

use super::record_finding;
use crate::index::identities::IdentitySet;

use crate::document::design_records::DesignRecords;
use crate::document::CadIr;
use crate::report::{
    check::{Check, Finding},
    Severity,
};

pub(super) fn check_semantic_annotations(
    ir: &CadIr,
    all_ids: &dyn IdentitySet,
    findings: &mut Vec<Finding>,
) {
    if ir.model.semantic_annotations.is_empty() {
        return;
    }
    let records = DesignRecords::of(ir);
    let named = crate::references::ReferenceRecords::of(&ir.model);
    for annotation in &ir.model.semantic_annotations {
        let native = |target: &str| {
            super::annotations_native::native_link_resolves(ir, |id| all_ids.contains(id), target)
        };
        let mut refs_valid = native(&annotation.object) && native(&annotation.native_ref);
        if refs_valid {
            for id in &annotation.assets {
                if !native(id) {
                    refs_valid = false;
                    break;
                }
            }
        }
        if refs_valid {
            'groups: for targets in annotation.references.values() {
                for target in targets {
                    if !target.resolves(&records, &named, |id| all_ids.contains(id)) {
                        refs_valid = false;
                        break 'groups;
                    }
                }
            }
        }
        if !refs_valid {
            record_finding(
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                Some(annotation.id.as_str()),
                format_args!("invalid semantic annotation reference"),
            );
        }
        check_reference_anchors(annotation, findings);
    }
}

/// The count of an annotation's page-space reference anchors, "none or one
/// pair" (§8): more than two is an Error; one, or two outside a dimension of
/// a stated kind, is a Warning.
fn check_reference_anchors(
    annotation: &crate::semantic_annotations::SemanticAnnotation,
    findings: &mut Vec<Finding>,
) {
    let count = annotation.reference_anchors.len();
    let severity = match count {
        0 => return,
        1 => Severity::Warning,
        2 if annotation.kind == crate::semantic_annotations::SemanticAnnotationKind::Dimension
            && annotation.dimension_kind.is_some() =>
        {
            return
        }
        2 => Severity::Warning,
        _ => Severity::Error,
    };
    record_finding(
        findings,
        Check::ReferentialIntegrity,
        severity,
        Some(annotation.id.as_str()),
        format_args!("semantic annotation states {count} reference anchors"),
    );
}

#[cfg(test)]
mod tests {
    use super::check_semantic_annotations;
    use crate::document::CadIr;

    fn fixture() -> CadIr {
        let mut ir = CadIr::empty();
        for id in [
            "test:model:semantic-annotation#first",
            "test:model:semantic-annotation#second",
        ] {
            ir.model.semantic_annotations.push(
                serde_json::from_value(serde_json::json!({
                    "id": id, "object": "test:source:unknown#record", "kind": "text",
                    "runtime_type": "Test", "order": 7, "native_ref": "test:source:unknown#record",
                    "text": [], "references": {}, "reference_anchors": [], "text_style": {},
                    "parameters": {}, "assets": []
                }))
                .unwrap(),
            );
        }
        ir
    }

    #[test]
    fn a_native_link_of_a_format_the_document_does_not_hold_is_provenance() {
        let mut ir = fixture();
        ir.model.semantic_annotations.truncate(1);
        let empty = crate::index::identities::BorrowedIdentities::build(|add| {
            add("test:model:unrelated#record", ());
        });
        let mut findings = Vec::new();
        check_semantic_annotations(&ir, &empty, &mut findings);
        assert!(findings.is_empty(), "{findings:?}");
        ir.native.namespace_mut(crate::codec_id!("test"));
        check_semantic_annotations(&ir, &empty, &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn dimension_fields_and_zero_tolerances_are_admitted_on_any_kind() {
        let mut ir = fixture();
        ir.model.semantic_annotations.truncate(1);
        let ids = crate::index::identities::BorrowedIdentities::build(|add| {
            add("test:source:unknown#record", ());
        });
        let annotation = &mut ir.model.semantic_annotations[0];
        annotation.upper_tolerance = Some(crate::scalar::FiniteReal::new(0.0).unwrap());
        annotation.lower_tolerance = Some(crate::scalar::FiniteReal::new(-0.1).unwrap());
        annotation.text_override = Some("R12".to_owned());
        let mut findings = Vec::new();
        check_semantic_annotations(&ir, &ids, &mut findings);
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn reference_anchors_are_none_or_one_pair() {
        use crate::report::Severity;
        use crate::semantic_annotations::{AnnotationDimensionKind, SemanticAnnotationKind};
        let mut ir = fixture();
        ir.model.semantic_annotations.truncate(1);
        let ids = crate::index::identities::BorrowedIdentities::build(|add| {
            add("test:source:unknown#record", ());
        });
        let anchor = crate::units::FiniteVector::new([35.0, 190.0]).unwrap();
        let severities = |ir: &CadIr| {
            let mut findings = Vec::new();
            check_semantic_annotations(ir, &ids, &mut findings);
            findings
                .iter()
                .map(|finding| finding.severity)
                .collect::<Vec<_>>()
        };
        for (count, expected) in [
            (0, Vec::new()),
            (1, vec![Severity::Warning]),
            (2, vec![Severity::Warning]),
            (3, vec![Severity::Error]),
        ] {
            ir.model.semantic_annotations[0].reference_anchors = vec![anchor; count];
            assert_eq!(severities(&ir), expected, "{count} anchors");
        }
        ir.model.semantic_annotations[0].reference_anchors = vec![anchor; 2];
        ir.model.semantic_annotations[0].kind = SemanticAnnotationKind::Dimension;
        ir.model.semantic_annotations[0].dimension_kind =
            Some(AnnotationDimensionKind::HorizontalDistance);
        assert_eq!(severities(&ir), Vec::new());
    }

    #[test]
    fn semantic_annotations_may_share_an_order() {
        let ir = fixture();
        let ids = crate::index::identities::BorrowedIdentities::build(|add| {
            add("test:source:unknown#record", ());
        });
        let mut findings = Vec::new();
        check_semantic_annotations(&ir, &ids, &mut findings);
        assert!(findings.is_empty(), "{findings:?}");
    }
}
