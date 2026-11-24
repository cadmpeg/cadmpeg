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
        check_dimension_kind(annotation, findings);
        if !refs_valid {
            record_finding(
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                Some(annotation.id.as_str()),
                crate::rule!(
                    "semantic_annotations.invalid-semantic-annotation-reference",
                    "invalid semantic annotation reference"
                ),
            );
        }
        check_reference_anchors(annotation, findings);
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
    let mut report = |ruled: crate::report::check::Ruled<'_>| {
        record_finding(
            findings,
            Check::ReferentialIntegrity,
            Severity::Error,
            Some(annotation.id.as_str()),
            ruled,
        );
    };
    if annotation.reflex.is_some() && !angle {
        report(crate::rule!(
            "semantic_annotations.reflex-without-angle-kind",
            "semantic annotation states `reflex`, but its dimension kind is no angle"
        ));
    }
    if annotation.negated.is_some() && angle {
        report(crate::rule!(
            "semantic_annotations.negated-with-angle-kind",
            "semantic annotation states `negated`, but its dimension kind is an angle"
        ));
    }
    if annotation.line_directions.is_some() && !distance {
        report(crate::rule!(
            "semantic_annotations.line-directions-without-distance-kind",
            "semantic annotation states `line_directions`, but its dimension kind is no distance"
        ));
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
        crate::rule!(
            "semantic_annotations.semantic-annotation-states-reference-anchors",
            "semantic annotation states {count} reference anchors"
        ),
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
