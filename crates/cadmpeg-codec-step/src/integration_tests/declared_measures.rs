// SPDX-License-Identifier: Apache-2.0
//! CAx-IF geometric validation properties of synthesized STEP parts,
//! compared with the measured geometry of the decoded bodies. A declared
//! value agrees when it lies within the measured value's numeric error plus
//! model uncertainty; the expected values below are the exact volumes,
//! areas and centroids of the boxes.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::report::check::{Check, Finding};
use cadmpeg_ir::report::Severity;
use cadmpeg_measure::MeasureOptions;

use crate::test_support::exchange::decode_inline;
use crate::test_support::validated_part::{part, validation_property, Item, Part, PartBody};

const CUBE: PartBody = PartBody::Solid {
    min: [0.0; 3],
    max: [10.0; 3],
};

/// The box `20..40 × 0..10 × 0..10`: volume 2000, area 1000, centroid
/// `(30, 5, 5)`.
const SLAB: PartBody = PartBody::Solid {
    min: [20.0, 0.0, 0.0],
    max: [40.0, 10.0, 10.0],
};

/// The faces of the box `20..30 × 0..10 × 0..10` but its face at `z = 10`,
/// as a sheet: five faces of area 100 centred at `(20, 5, 5)`,
/// `(30, 5, 5)`, `(25, 0, 5)`, `(25, 10, 5)` and `(25, 5, 0)`, so area 500
/// and area centroid `(25, 5, 4)`.
const SHEET: PartBody = PartBody::Sheet {
    min: [20.0, 0.0, 0.0],
    max: [30.0, 10.0, 10.0],
};

/// The declared-measure findings of `part` with one validation property on
/// `definition` holding `items`, and `extra` records.
fn findings(part: &Part, definition: u64, items: &[Item], extra: &str) -> Vec<Finding> {
    let validation = validation_property(part, 1000, definition, items);
    let result = decode_inline(&format!("{}{extra}{validation}", part.records));
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).expect("context");
    let findings = cadmpeg_measure::declared_findings(&ctx, result.ir(), MeasureOptions::default())
        .expect("declared findings");
    ctx.finish_session().expect("session stays unrefused");
    for finding in &findings {
        assert_eq!(finding.check, Check::DeclaredMeasure, "{finding:?}");
    }
    findings
}

fn severities(findings: &[Finding]) -> Vec<Severity> {
    findings.iter().map(|finding| finding.severity).collect()
}

#[test]
fn exact_part_properties_agree() {
    let part = part(&[CUBE]);
    let found = findings(
        &part,
        part.shape,
        &[
            Item::Volume("volume measure", 1000.0),
            Item::Area("surface area measure", 600.0),
            Item::Area("wetted area measure", 600.0),
            Item::Point("centre point", [5.0, 5.0, 5.0]),
        ],
        "",
    );
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn a_value_beyond_the_measured_error_terms_disagrees() {
    let part = part(&[CUBE]);
    let found = findings(
        &part,
        part.shape,
        &[
            Item::Volume("volume measure", 1001.0),
            Item::Area("surface area measure", 601.0),
            Item::Point("centre point", [5.0, 5.0, 6.0]),
        ],
        "",
    );
    assert_eq!(severities(&found), [Severity::Error; 3], "{found:#?}");
    let body = format!("step:data:body#{}", part.items[0]);
    for finding in &found {
        assert_eq!(finding.related, [body.clone()]);
    }
}

#[test]
fn a_part_property_sums_every_solid_body() {
    let part = part(&[CUBE, SLAB]);
    let total = [
        Item::Volume("volume measure", 3000.0),
        Item::Area("surface area measure", 1600.0),
        Item::Point("centre point", [65.0 / 3.0, 5.0, 5.0]),
    ];
    let found = findings(&part, part.shape, &total, "");
    assert!(found.is_empty(), "{found:#?}");
    let first_only = findings(
        &part,
        part.shape,
        &[Item::Volume("volume measure", 1000.0)],
        "",
    );
    assert_eq!(
        severities(&first_only),
        [Severity::Error],
        "{first_only:#?}"
    );
    assert_eq!(first_only[0].related.len(), 2);
}

#[test]
fn a_geometry_level_property_covers_only_its_bodies() {
    let part = part(&[CUBE, SLAB]);
    let aspect = format!(
        "#900=SHAPE_ASPECT('','slab',#{},.F.);\n\
         #901=GEOMETRIC_ITEM_SPECIFIC_USAGE('','shape for validation properties',#900,#{},#{});\n",
        part.shape, part.representation, part.items[1]
    );
    let found = findings(
        &part,
        900,
        &[
            Item::Volume("volume measure", 2000.0),
            Item::Area("surface area measure", 1000.0),
            Item::Point("centre point", [30.0, 5.0, 5.0]),
        ],
        &aspect,
    );
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn sheet_properties_cover_the_sheets_and_solid_properties_the_solids() {
    let part = part(&[CUBE, SHEET]);
    let found = findings(
        &part,
        part.shape,
        &[
            Item::Volume("volume measure", 1000.0),
            Item::Area("surface area measure", 600.0),
            Item::Point("centre point", [5.0, 5.0, 5.0]),
            Item::Area("independent surface area measure", 500.0),
            Item::Point("surface centre point", [25.0, 5.0, 4.0]),
        ],
        "",
    );
    assert!(found.is_empty(), "{found:#?}");
    let combined = findings(
        &part,
        part.shape,
        &[Item::Area("independent surface area measure", 1100.0)],
        "",
    );
    assert_eq!(severities(&combined), [Severity::Error], "{combined:#?}");
}

#[test]
fn an_unmeasurable_quantity_is_a_warning_with_its_reason() {
    let part = part(&[SHEET]);
    let aspect = format!(
        "#900=SHAPE_ASPECT('','sheet',#{},.F.);\n\
         #901=GEOMETRIC_ITEM_SPECIFIC_USAGE('','shape for validation properties',#900,#{},#{});\n",
        part.shape, part.representation, part.items[0]
    );
    let found = findings(
        &part,
        900,
        &[Item::Volume("volume measure", 1000.0)],
        &aspect,
    );
    assert_eq!(severities(&found), [Severity::Warning], "{found:#?}");
    assert!(
        found[0].message.contains("(volume.shell-open)"),
        "{found:#?}"
    );
}

#[test]
fn an_unresolved_target_is_a_warning() {
    let part = part(&[CUBE]);
    let found = findings(
        &part,
        900,
        &[Item::Volume("volume measure", 1000.0)],
        &format!("#900=SHAPE_ASPECT('','no geometry',#{},.F.);\n", part.shape),
    );
    assert_eq!(severities(&found), [Severity::Warning], "{found:#?}");
    assert!(
        found[0].message.contains("(declared.target-unresolved)"),
        "{found:#?}"
    );
}

#[test]
fn a_geometry_level_area_for_the_wrong_class_warns() {
    let part = part(&[CUBE]);
    let aspect = format!(
        "#900=SHAPE_ASPECT('','solid',#{},.F.);\n\
         #901=GEOMETRIC_ITEM_SPECIFIC_USAGE('','shape for validation properties',#900,#{},#{});\n",
        part.shape, part.representation, part.items[0]
    );
    let found = findings(
        &part,
        900,
        &[Item::Area("independent surface area measure", 600.0)],
        &aspect,
    );
    assert_eq!(severities(&found), [Severity::Warning], "{found:#?}");
    assert!(found[0].message.contains("declared.geometry-class"));
}
