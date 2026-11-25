// SPDX-License-Identifier: Apache-2.0
//! Drawing graph and numeric validation.

use super::record_finding;
use crate::index::identities::IdentitySet;

use std::collections::{BTreeMap, BTreeSet};

use crate::document::design_records::DesignRecords;
use crate::document::CadIr;
use crate::drawings::{Drawing, DrawingId, DrawingKind};
use crate::references::ReferenceRecords;
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use crate::validate::cycles::cycles;

pub(super) fn check_drawings(ir: &CadIr, all_ids: &dyn IdentitySet, findings: &mut Vec<Finding>) {
    if ir.model.drawings.is_empty() {
        return;
    }
    let records = DesignRecords::of(ir);
    let named = ReferenceRecords::of(&ir.model);
    let cyclic = on_cycles(&ir.model.drawings);
    let mut orders = std::collections::BTreeSet::new();
    for drawing in &ir.model.drawings {
        let mut refs_valid = all_ids.contains(&drawing.object)
            && drawing
                .native_ref
                .as_deref()
                .is_none_or(|native| all_ids.contains(native));
        if refs_valid {
            if let Some(id) = &drawing.template {
                refs_valid = all_ids.contains(id.as_str());
            }
        }
        if refs_valid {
            for id in &drawing.assets {
                if !all_ids.contains(id) {
                    refs_valid = false;
                    break;
                }
            }
        }
        if refs_valid {
            'groups: for targets in drawing.relationships.values() {
                for target in targets {
                    if !target.resolves(&records, &named, |id| all_ids.contains(id)) {
                        refs_valid = false;
                        break 'groups;
                    }
                }
            }
        }
        if !views_are_valid(drawing, &named) || cyclic.contains(drawing.id.as_str()) {
            record_finding(
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                Some(drawing.id.as_str()),
                crate::rule!("drawings.drawing-views-base-view-name-record", 
                    "drawing views or base view name a record of another kind, repeat, or form a cycle"
                ),
            );
        }
        let order_valid = orders.insert(drawing.order);
        let numeric_valid = drawing.physical_content_is_valid()
            && drawing
                .sheet
                .as_ref()
                .is_none_or(crate::drawings::DrawingSheet::is_valid)
            && drawing
                .curves
                .iter()
                .all(crate::drawings::DrawingCurve::is_valid);
        if !refs_valid || !order_valid || !numeric_valid {
            record_finding(
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                Some(drawing.id.as_str()),
                crate::rule!(
                    "drawings.invalid-drawing-reference-order-numeric-state",
                    "invalid drawing reference, order, or numeric state"
                ),
            );
        }
    }
}

/// Whether each `views` entry names a drawing record that is no `Page` and
/// no `Template`, once, and `base_view` names a `View`, `Projection`,
/// `Section`, or `Detail` record.
fn views_are_valid(drawing: &Drawing, targets: &ReferenceRecords<'_>) -> bool {
    let mut listed = BTreeSet::new();
    drawing.views.iter().all(|view| {
        listed.insert(view.as_str())
            && targets.drawing(view).is_some_and(|record| {
                !matches!(record.kind, DrawingKind::Page | DrawingKind::Template)
            })
    }) && drawing.base_view.as_ref().is_none_or(|base| {
        targets.drawing(base).is_some_and(|record| {
            matches!(
                record.kind,
                DrawingKind::View
                    | DrawingKind::Projection
                    | DrawingKind::Section
                    | DrawingKind::Detail
            )
        })
    })
}

/// The ids of the drawing records reached from themselves through `views`,
/// or through `base_view`.
fn on_cycles(drawings: &[Drawing]) -> BTreeSet<&str> {
    let ids: BTreeSet<&str> = drawings.iter().map(|drawing| drawing.id.as_str()).collect();
    let graph = |links: fn(&Drawing) -> Vec<&DrawingId>| {
        drawings
            .iter()
            .map(|drawing| {
                let targets = links(drawing)
                    .into_iter()
                    .map(DrawingId::as_str)
                    .filter(|target| ids.contains(target))
                    .collect::<BTreeSet<_>>();
                (drawing.id.as_str(), targets)
            })
            .collect::<BTreeMap<_, _>>()
    };
    let through_views = graph(|drawing| drawing.views.iter().collect());
    let through_bases = graph(|drawing| drawing.base_view.iter().collect());
    cycles(&through_views)
        .into_iter()
        .chain(cycles(&through_bases))
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::check_drawings;
    use crate::document::CadIr;

    fn fixture() -> CadIr {
        let mut ir = CadIr::empty();
        for id in ["test:model:drawing#first", "test:model:drawing#second"] {
            ir.model.drawings.push(
                cadmpeg_core::serde_erase::json::from_value(serde_json::json!({
                    "id": id, "object": "test:source:unknown#record", "kind": "page",
                    "runtime_type": "Test", "order": 7, "native_ref": "test:source:unknown#record",
                    "relationships": {}, "views": [], "curves": [], "parameters": {}, "assets": []
                }))
                .unwrap(),
            );
        }
        ir
    }

    #[test]
    fn validator_checks_sheet_names_and_arc_form() {
        let mut ir = fixture();
        ir.model.drawings.truncate(1);
        let ids = crate::index::identities::BorrowedIdentities::build(|add| {
            add("test:source:unknown#record", ());
        });
        let sheet = serde_json::json!({"width_mm": 20.0, "height_mm": 30.0, "fields": [{"name": "Title", "value": "A"}, {"name": "Title", "value": "B"}]});
        ir.model.drawings[0].sheet =
            Some(cadmpeg_core::serde_erase::json::from_value(sheet).unwrap());
        let mut findings = Vec::new();
        check_drawings(&ir, &ids, &mut findings);
        assert_eq!(findings.len(), 1);
        ir.model.drawings[0].sheet.as_mut().unwrap().fields.pop();
        findings.clear();
        check_drawings(&ir, &ids, &mut findings);
        assert!(findings.is_empty());
        ir.model.drawings[0].curves.push(cadmpeg_core::serde_erase::json::from_value(serde_json::json!({"geometry": {"kind": "arc", "center": [0, 0], "radius": 1, "start": [1, 0], "middle": [1, 0], "end": [0, 1]}, "visibility": "visible", "source": "cosmetic"})).unwrap());
        check_drawings(&ir, &ids, &mut findings);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn template_geometry_and_horizontal_axis_require_their_owners() {
        use crate::drawings::{DrawingKind, TemplateGeometry};
        let mut ir = fixture();
        ir.model.drawings.truncate(1);
        let ids = crate::index::identities::BorrowedIdentities::build(|add| {
            add("test:source:unknown#record", ());
        });
        let template = cadmpeg_core::serde_erase::json::from_value::<TemplateGeometry>(serde_json::json!({
            "curves": [{"kind":"line", "start":[10,287], "end":[200,287]},
                       {"kind":"circle", "center":[50,247], "radius":10},
                       {"kind":"arc", "center":[20,267], "radius":10, "start":[20,277], "middle":[30,267], "end":[20,257]}],
            "texts": [{"text":"Title", "position":[115,19], "height":4}]
        })).unwrap();
        ir.model.drawings[0].template_geometry = Some(template);
        let mut findings = Vec::new();
        check_drawings(&ir, &ids, &mut findings);
        assert_eq!(findings.len(), 1);
        ir.model.drawings[0].kind = DrawingKind::Template;
        findings.clear();
        check_drawings(&ir, &ids, &mut findings);
        assert!(findings.is_empty());
        if let crate::drawings::DrawingCurveGeometry::Arc { middle, start, .. } =
            &mut ir.model.drawings[0]
                .template_geometry
                .as_mut()
                .unwrap()
                .curves[2]
        {
            *middle = *start;
        }
        check_drawings(&ir, &ids, &mut findings);
        assert_eq!(findings.len(), 1);
        ir.model.drawings[0].template_geometry = None;
        ir.model.drawings[0].x_direction = crate::units::NonzeroVector::new([1.0, 0.0, 0.0]);
        findings.clear();
        check_drawings(&ir, &ids, &mut findings);
        assert_eq!(findings.len(), 1);
        for kind in [
            DrawingKind::View,
            DrawingKind::Projection,
            DrawingKind::Section,
            DrawingKind::Detail,
        ] {
            ir.model.drawings[0].kind = kind;
            findings.clear();
            check_drawings(&ir, &ids, &mut findings);
            assert!(findings.is_empty());
        }
    }

    #[test]
    fn a_drawing_resolves_a_feature_in_either_home() {
        use crate::features::FeatureId;
        use crate::references::{ReferenceSelection, ReferenceTarget};

        let id = FeatureId::mint("test:design:feature#cut").unwrap();
        let ids = crate::index::identities::BorrowedIdentities::build(|add| {
            add("test:source:unknown#record", ());
        });
        let findings = |ir: &CadIr| {
            let mut findings = Vec::new();
            check_drawings(ir, &ids, &mut findings);
            findings.len()
        };
        let mut ir = fixture();
        ir.model.drawings.truncate(1);
        ir.model.drawings[0].relationships.insert(
            cadmpeg_core::text::NonBlankString::new("Source".to_owned()).unwrap(),
            vec![ReferenceSelection::new(ReferenceTarget::Feature(
                id.clone(),
            ))],
        );
        assert_eq!(findings(&ir), 1);

        let mut model = ir.clone();
        model.model.features.push(crate::features::Feature {
            product_definition: None,
            id: id.clone(),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: crate::features::selections::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: crate::features::FeatureContent::default(),
            evaluation: crate::features::evaluation::FeatureEvaluation::from_definition(
                crate::features::FeatureDefinition::Operation(
                    crate::features::FeatureOperation::Native {
                        kind: "Cut".into(),
                        parameters: std::collections::BTreeMap::new(),
                    },
                ),
            ),
            native_ref: None,
        });
        assert_eq!(findings(&model), 0);

        let mut design = ir;
        design
            .design
            .features
            .push(crate::design::feature::Feature {
                id,
                name: None,
                suppressed: None,
                definition: crate::design::feature::FeatureDefinition {
                    operation: crate::design::feature::FeatureOperation::Unresolved {
                        family: crate::design::feature::OperationFamily::Extrude,
                    },
                    assembly: None,
                    refine: None,
                    fuzzy_tolerance: None,
                },
                placement: None,
                component: None,
                driven_by: None,
                native_ref: None,
            });
        assert_eq!(findings(&design), 0);
    }

    /// Part 10: an `Unresolved` reference target carries `native`.
    #[test]
    fn an_unresolved_reference_carries_the_sources_name() {
        use crate::references::{ReferenceSelection, ReferenceTarget};

        let ids = crate::index::identities::BorrowedIdentities::build(|add| {
            add("test:source:unknown#record", ());
        });
        let with = |reference: ReferenceSelection| {
            let mut ir = fixture();
            ir.model.drawings.truncate(1);
            ir.model.drawings[0].relationships.insert(
                cadmpeg_core::text::NonBlankString::new("Source".to_owned()).unwrap(),
                vec![reference],
            );
            let mut findings = Vec::new();
            check_drawings(&ir, &ids, &mut findings);
            findings.len()
        };
        let native = cadmpeg_core::serde_erase::json::from_value(
            serde_json::json!({"codec": "fcstd", "value": {"kind": "text", "value": "View.Edge1"}}),
        )
        .unwrap();
        assert_eq!(with(ReferenceSelection::unresolved(native)), 0);
        assert_eq!(
            with(ReferenceSelection::new(ReferenceTarget::Unresolved)),
            1
        );
    }

    fn graph(kinds: &[(&str, crate::drawings::DrawingKind)]) -> CadIr {
        let mut ir = CadIr::empty();
        for (order, (name, kind)) in kinds.iter().enumerate() {
            ir.model.drawings.push(crate::drawings::Drawing::new(
                drawing_id(name),
                "test:source:unknown#record".to_owned(),
                kind.clone(),
                "Test".to_owned(),
                u32::try_from(order).unwrap(),
                None,
            ));
        }
        ir
    }

    fn drawing_id(name: &str) -> crate::drawings::DrawingId {
        crate::drawings::DrawingId::mint(format!("test:model:drawing#{name}")).unwrap()
    }

    fn flagged(ir: &CadIr) -> Vec<String> {
        let ids = crate::index::identities::BorrowedIdentities::build(|add| {
            add("test:source:unknown#record", ());
        });
        let mut findings = Vec::new();
        check_drawings(ir, &ids, &mut findings);
        findings
            .into_iter()
            .map(|finding| finding.entity.unwrap_or_default())
            .collect()
    }

    #[test]
    fn views_name_records_that_are_no_page_or_template_once() {
        use crate::drawings::DrawingKind;
        let mut ir = graph(&[
            ("page", DrawingKind::Page),
            ("template", DrawingKind::Template),
            ("group", DrawingKind::Projection),
            ("item", DrawingKind::Projection),
            ("clip", DrawingKind::View),
            ("other", DrawingKind::Page),
        ]);
        ir.model.drawings[0].views = vec![drawing_id("group"), drawing_id("clip")];
        ir.model.drawings[2].views = vec![drawing_id("item")];
        ir.model.drawings[4].views = vec![drawing_id("item")];
        assert!(flagged(&ir).is_empty(), "{:?}", flagged(&ir));
        for invalid in ["other", "template", "absent"] {
            let mut bad = ir.clone();
            bad.model.drawings[2].views.push(drawing_id(invalid));
            assert_eq!(flagged(&bad), ["test:model:drawing#group"], "{invalid}");
        }
        let mut repeated = ir.clone();
        repeated.model.drawings[0].views.push(drawing_id("group"));
        assert_eq!(flagged(&repeated), ["test:model:drawing#page"]);
        let mut cycle = ir;
        cycle.model.drawings[3].views = vec![drawing_id("clip")];
        cycle.model.drawings[4].views = vec![drawing_id("group")];
        assert_eq!(
            flagged(&cycle),
            [
                "test:model:drawing#group",
                "test:model:drawing#item",
                "test:model:drawing#clip"
            ]
        );
    }

    #[test]
    fn a_base_view_names_a_view_kind_and_base_views_form_no_cycle() {
        use crate::drawings::DrawingKind;
        let mut ir = graph(&[
            ("front", DrawingKind::View),
            ("section", DrawingKind::Section),
            ("detail", DrawingKind::Detail),
            ("note", DrawingKind::Annotation),
        ]);
        ir.model.drawings[1].base_view = Some(drawing_id("front"));
        ir.model.drawings[2].base_view = Some(drawing_id("section"));
        assert!(flagged(&ir).is_empty());
        let mut annotation = ir.clone();
        annotation.model.drawings[2].base_view = Some(drawing_id("note"));
        assert_eq!(flagged(&annotation), ["test:model:drawing#detail"]);
        let mut itself = ir.clone();
        itself.model.drawings[0].base_view = Some(drawing_id("front"));
        assert_eq!(flagged(&itself), ["test:model:drawing#front"]);
        let mut cycle = ir;
        cycle.model.drawings[0].base_view = Some(drawing_id("detail"));
        assert_eq!(
            flagged(&cycle),
            [
                "test:model:drawing#front",
                "test:model:drawing#section",
                "test:model:drawing#detail"
            ]
        );
    }

    #[test]
    fn drawing_annotation_and_pmi_targets_name_existing_records() {
        use crate::drawings::DrawingKind;
        use crate::references::{DrawingPart, ReferenceSelection, ReferenceTarget};
        let mut ir = graph(&[
            ("dimension", DrawingKind::Dimension),
            ("front", DrawingKind::View),
        ]);
        ir.model.drawings[1]
            .curves
            .push(cadmpeg_core::serde_erase::json::from_value(serde_json::json!({"geometry": {"kind": "line", "start": [0, 0], "end": [1, 0]}, "visibility": "visible", "source": "cosmetic"})).unwrap());
        let with_target = |ir: &CadIr, target: ReferenceTarget| {
            let mut ir = ir.clone();
            ir.model.drawings[0].relationships.insert(
                cadmpeg_core::text::NonBlankString::new("References2D".to_owned()).unwrap(),
                vec![ReferenceSelection::new(target)],
            );
            flagged(&ir)
        };
        let view = |part| ReferenceTarget::Drawing {
            drawing: drawing_id("front"),
            part,
        };
        assert!(with_target(&ir, view(None)).is_empty());
        assert!(with_target(&ir, view(Some(DrawingPart::Unread))).is_empty());
        assert!(with_target(&ir, view(Some(DrawingPart::Curve(0)))).is_empty());
        assert_eq!(
            with_target(&ir, view(Some(DrawingPart::Curve(1)))),
            ["test:model:drawing#dimension"]
        );
        assert_eq!(
            with_target(
                &ir,
                ReferenceTarget::Drawing {
                    drawing: drawing_id("absent"),
                    part: None
                }
            ),
            ["test:model:drawing#dimension"]
        );
        let annotation = crate::semantic_annotations::SemanticAnnotationId::mint(
            "test:model:semantic-annotation#note",
        )
        .unwrap();
        assert_eq!(
            with_target(&ir, ReferenceTarget::Annotation(annotation.clone())),
            ["test:model:drawing#dimension"]
        );
        let pmi = crate::ids::PmiId::mint("test:model:pmi#flatness").unwrap();
        assert_eq!(
            with_target(&ir, ReferenceTarget::Pmi(pmi.clone())),
            ["test:model:drawing#dimension"]
        );
        ir.model
            .semantic_annotations
            .push(crate::semantic_annotations::SemanticAnnotation::new(
                annotation.clone(),
                "test:source:unknown#record".to_owned(),
                crate::semantic_annotations::SemanticAnnotationKind::Text,
                "Test".to_owned(),
                0,
                "test:source:unknown#record".to_owned(),
            ));
        assert!(with_target(&ir, ReferenceTarget::Annotation(annotation)).is_empty());
        ir.model.pmi.push(crate::pmi::PmiAnnotation::new(
            pmi.clone(),
            Vec::new(),
            crate::pmi::PmiDefinition::Presentation {
                text: None,
                placement: None,
                semantics: Vec::new(),
            },
        ));
        assert!(with_target(&ir, ReferenceTarget::Pmi(pmi)).is_empty());
    }

    #[test]
    fn drawings_report_the_second_duplicate_order() {
        let ir = fixture();
        let ids = crate::index::identities::BorrowedIdentities::build(|add| {
            add("test:source:unknown#record", ());
        });
        let mut findings = Vec::new();
        check_drawings(&ir, &ids, &mut findings);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].entity.as_deref(),
            Some("test:model:drawing#second")
        );
        assert_eq!(
            findings[0].check,
            crate::report::check::Check::ReferentialIntegrity
        );
        assert_eq!(
            findings[0].message,
            "invalid drawing reference, order, or numeric state"
        );
    }
}
