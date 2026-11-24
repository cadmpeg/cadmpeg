// SPDX-License-Identifier: Apache-2.0
//! Semantic annotation graph and numeric validation.

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
    let mut orders = std::collections::BTreeSet::new();
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
        let order_valid = orders.insert(annotation.order);
        let has_dimension_fields = annotation.dimension_kind.is_some()
            || annotation.text_override.is_some()
            || annotation.upper_tolerance.is_some()
            || annotation.lower_tolerance.is_some();
        let numeric_valid = annotation.reference_anchors_are_valid()
            && (!has_dimension_fields
                || annotation.kind
                    == crate::semantic_annotations::SemanticAnnotationKind::Dimension)
            && [annotation.upper_tolerance, annotation.lower_tolerance]
                .iter()
                .flatten()
                .all(|value| value.get().is_finite() && value.get() != 0.0);
        check_dimension_kind(annotation, findings);
        if !refs_valid || !order_valid || !numeric_valid {
            record_finding(
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                Some(annotation.id.as_str()),
                format_args!("invalid semantic annotation reference, order, or numeric state"),
            );
        }
    }
}

/// `reflex` goes with an angle kind, `negated` with a kind that is not an
/// angle, and `line_directions` with a distance kind (part 10, §8). An
/// `Other` kind states no category these read.
fn check_dimension_kind(
    annotation: &crate::semantic_annotations::SemanticAnnotation,
    findings: &mut Vec<Finding>,
) {
    use crate::semantic_annotations::AnnotationDimensionKind as Kind;
    let Some(kind) = &annotation.dimension_kind else {
        return;
    };
    if matches!(kind, Kind::Other { .. }) {
        return;
    }
    let angle = matches!(kind, Kind::Angle | Kind::ThreePointAngle);
    let distance = matches!(
        kind,
        Kind::Distance | Kind::HorizontalDistance | Kind::VerticalDistance
    );
    for (stated, admitted, message) in [
        (
            annotation.reflex.is_some(),
            angle,
            "semantic annotation states `reflex`, but its dimension kind is no angle",
        ),
        (
            annotation.negated.is_some(),
            !angle,
            "semantic annotation states `negated`, but its dimension kind is an angle",
        ),
        (
            annotation.line_directions.is_some(),
            distance,
            "semantic annotation states `line_directions`, but its dimension kind is no distance",
        ),
    ] {
        if stated && !admitted {
            record_finding(
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                Some(annotation.id.as_str()),
                format_args!("{message}"),
            );
        }
    }
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
    fn validator_requires_dimension_owner_and_set_tolerance() {
        let mut ir = fixture();
        ir.model.semantic_annotations.truncate(1);
        let ids = crate::index::identities::BorrowedIdentities::build(|add| {
            add("test:source:unknown#record", ());
        });
        ir.model.semantic_annotations[0].upper_tolerance =
            Some(crate::scalar::FiniteReal::new(0.2).unwrap());
        let mut findings = Vec::new();
        check_semantic_annotations(&ir, &ids, &mut findings);
        assert_eq!(findings.len(), 1);
        ir.model.semantic_annotations[0].kind =
            crate::semantic_annotations::SemanticAnnotationKind::Dimension;
        findings.clear();
        check_semantic_annotations(&ir, &ids, &mut findings);
        assert!(findings.is_empty());
        ir.model.semantic_annotations[0].upper_tolerance =
            Some(crate::scalar::FiniteReal::new(0.0).unwrap());
        check_semantic_annotations(&ir, &ids, &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn reference_anchors_require_a_typed_dimension_and_one_pair() {
        use crate::semantic_annotations::{AnnotationDimensionKind, SemanticAnnotationKind};
        let mut ir = fixture();
        ir.model.semantic_annotations.truncate(1);
        let ids = crate::index::identities::BorrowedIdentities::build(|add| {
            add("test:source:unknown#record", ());
        });
        let pair = vec![
            crate::units::FiniteVector::new([35.0, 190.0]).unwrap(),
            crate::units::FiniteVector::new([95.0, 190.0]).unwrap(),
        ];
        let annotation = &mut ir.model.semantic_annotations[0];
        annotation.reference_anchors = pair.clone();
        let mut findings = Vec::new();
        check_semantic_annotations(&ir, &ids, &mut findings);
        assert_eq!(findings.len(), 1);
        ir.model.semantic_annotations[0].kind = SemanticAnnotationKind::Dimension;
        findings.clear();
        check_semantic_annotations(&ir, &ids, &mut findings);
        assert_eq!(findings.len(), 1);
        ir.model.semantic_annotations[0].dimension_kind =
            Some(AnnotationDimensionKind::HorizontalDistance);
        findings.clear();
        check_semantic_annotations(&ir, &ids, &mut findings);
        assert!(findings.is_empty());
        for anchors in [
            vec![],
            pair[..1].to_vec(),
            pair.clone(),
            [pair.clone(), pair[..1].to_vec()].concat(),
        ] {
            let valid = anchors.is_empty() || anchors.len() == 2;
            ir.model.semantic_annotations[0].reference_anchors = anchors;
            findings.clear();
            check_semantic_annotations(&ir, &ids, &mut findings);
            assert_eq!(findings.is_empty(), valid);
        }
    }

    #[test]
    fn reflex_negated_and_line_directions_go_with_their_dimension_kinds() {
        use crate::semantic_annotations::{
            AnnotationDimensionKind as Kind, LineDirections, SemanticAnnotationKind,
        };
        let ids = crate::index::identities::BorrowedIdentities::build(|add| {
            add("test:source:unknown#record", ());
        });
        let messages = |kind: Kind, reflex: bool, negated: bool, lines: bool| {
            let mut ir = fixture();
            ir.model.semantic_annotations.truncate(1);
            let annotation = &mut ir.model.semantic_annotations[0];
            annotation.kind = SemanticAnnotationKind::Dimension;
            annotation.dimension_kind = Some(kind);
            annotation.reflex = reflex.then_some(true);
            annotation.negated = negated.then_some(false);
            annotation.line_directions = lines.then(|| LineDirections {
                dimension_line: crate::scalar::Angle::new(0.0).unwrap(),
                extension_lines: crate::scalar::Angle::new(1.5).unwrap(),
            });
            let mut findings = Vec::new();
            check_semantic_annotations(&ir, &ids, &mut findings);
            findings
                .into_iter()
                .map(|finding| finding.message)
                .collect::<Vec<_>>()
        };
        assert!(messages(Kind::Angle, true, false, false).is_empty());
        assert!(messages(Kind::Radius, false, true, false).is_empty());
        assert!(messages(Kind::VerticalDistance, false, true, true).is_empty());
        let other = Kind::Other {
            native_type: "test".to_owned(),
        };
        assert!(messages(other, true, true, true).is_empty());
        assert_eq!(
            messages(Kind::Radius, true, false, true),
            [
                "semantic annotation states `reflex`, but its dimension kind is no angle",
                "semantic annotation states `line_directions`, but its dimension kind is no distance",
            ]
        );
        assert_eq!(
            messages(Kind::ThreePointAngle, false, true, false),
            ["semantic annotation states `negated`, but its dimension kind is an angle"]
        );
    }

    #[test]
    fn semantic_annotations_report_the_second_duplicate_order() {
        let ir = fixture();
        let ids = crate::index::identities::BorrowedIdentities::build(|add| {
            add("test:source:unknown#record", ());
        });
        let mut findings = Vec::new();
        check_semantic_annotations(&ir, &ids, &mut findings);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].entity.as_deref(),
            Some("test:model:semantic-annotation#second")
        );
        assert_eq!(
            findings[0].check,
            crate::report::check::Check::ReferentialIntegrity
        );
        assert_eq!(
            findings[0].message,
            "invalid semantic annotation reference, order, or numeric state"
        );
    }
}
