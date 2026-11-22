// SPDX-License-Identifier: Apache-2.0
//! Drawing graph and numeric validation.

use super::record_finding;
use crate::index::identities::IdentitySet;

use crate::document::design_records::DesignRecords;
use crate::document::CadIr;
use crate::report::{
    check::{Check, Finding},
    Severity,
};

pub(super) fn check_drawings(ir: &CadIr, all_ids: &dyn IdentitySet, findings: &mut Vec<Finding>) {
    if ir.model.drawings.is_empty() {
        return;
    }
    let records = DesignRecords::of(ir);
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
                    if !target.target.resolves(&records, |id| all_ids.contains(id)) {
                        refs_valid = false;
                        break 'groups;
                    }
                }
            }
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
                format_args!("invalid drawing reference, order, or numeric state"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::check_drawings;
    use crate::document::CadIr;

    fn fixture() -> CadIr {
        let mut ir = CadIr::empty();
        for id in ["test:model:drawing#first", "test:model:drawing#second"] {
            ir.model.drawings.push(
                serde_json::from_value(serde_json::json!({
                    "id": id, "object": "test:source:unknown#record", "kind": "page",
                    "runtime_type": "Test", "order": 7, "native_ref": "test:source:unknown#record",
                    "relationships": {}, "curves": [], "parameters": {}, "assets": []
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
        ir.model.drawings[0].sheet = Some(serde_json::from_value(sheet).unwrap());
        let mut findings = Vec::new();
        check_drawings(&ir, &ids, &mut findings);
        assert_eq!(findings.len(), 1);
        ir.model.drawings[0].sheet.as_mut().unwrap().fields.pop();
        findings.clear();
        check_drawings(&ir, &ids, &mut findings);
        assert!(findings.is_empty());
        ir.model.drawings[0].curves.push(serde_json::from_value(serde_json::json!({"geometry": {"kind": "arc", "center": [0, 0], "radius": 1, "start": [1, 0], "middle": [1, 0], "end": [0, 1]}, "visibility": "visible", "source": "cosmetic"})).unwrap());
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
        let template = serde_json::from_value::<TemplateGeometry>(serde_json::json!({
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
