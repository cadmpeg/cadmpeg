// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use super::{code_name, Strictness};
use crate::report::loss::{LossKind, LossNote, LossTaxonomy, NamespacedLossKind};
use crate::report::Severity;

fn note(
    namespace: &'static str,
    code: &str,
    floor: Option<Severity>,
    severity: Severity,
) -> LossNote {
    let namespace = crate::report::loss::LossNamespace::new(namespace).expect("codec namespace");
    let kind = NamespacedLossKind::new(namespace, code, LossTaxonomy::DecodeDiagnostic)
        .with_strict_floor(floor);
    LossNote::new(kind, "message").with_severity(severity)
}

#[test]
fn the_default_refuses_nothing() {
    let strictness = Strictness::default();
    for severity in [
        Severity::Info,
        Severity::Warning,
        Severity::Error,
        Severity::Blocking,
    ] {
        for container in [false, true] {
            let pinned = note("iges", "a.b", Some(Severity::Info), severity);
            assert!(!strictness.refuses(&pinned, container));
        }
    }
}

#[test]
fn floor_refuses_an_entry_at_or_above_it() {
    let strictness = Strictness {
        floor: Some(Severity::Warning),
        ..Strictness::default()
    };
    assert!(!strictness.refuses(&note("iges", "a.b", None, Severity::Info), false));
    assert!(strictness.refuses(&note("iges", "a.b", None, Severity::Warning), false));
    assert!(strictness.refuses(&note("iges", "a.b", None, Severity::Blocking), true));
}

#[test]
fn codes_refuse_a_named_entry_whatever_its_severity() {
    let strictness = Strictness::default().with_code("export:record.invalid");
    let named = LossNote::new(
        LossKind::namespaced(
            crate::loss_namespace!("export"),
            "record.invalid",
            LossTaxonomy::DecodeDiagnostic,
        ),
        "message",
    )
    .with_severity(Severity::Info);
    assert!(strictness.refuses(&named, false));
    assert!(strictness.refuses(&named, true));
    assert!(!strictness.refuses(
        &note("iges", "record.invalid", None, Severity::Blocking),
        false
    ));
}

#[test]
fn pinned_refuses_an_entry_that_reaches_its_strict_floor_except_at_container_depth() {
    let strictness = Strictness {
        pinned: true,
        ..Strictness::default()
    };
    let reached = note("iges", "a.b", Some(Severity::Warning), Severity::Warning);
    assert!(strictness.refuses(&reached, false));
    assert!(!strictness.refuses(&reached, true));
    assert!(!strictness.refuses(
        &note("iges", "a.b", Some(Severity::Warning), Severity::Info),
        false
    ));
    assert!(!strictness.refuses(&note("iges", "a.b", None, Severity::Blocking), false));
}

#[test]
fn first_refused_names_the_first_refused_entry() {
    let entries = [
        note("iges", "a.b", None, Severity::Info),
        note("iges", "c.d", None, Severity::Error),
        note("iges", "e.f", None, Severity::Error),
    ];
    let strictness = Strictness {
        floor: Some(Severity::Error),
        ..Strictness::default()
    };
    assert_eq!(strictness.first_refused(&entries, false), Some(1));
    assert_eq!(Strictness::default().first_refused(&entries, false), None);
}

#[test]
fn a_blank_code_name_names_nothing() {
    assert_eq!(Strictness::default().with_code("  ").codes, BTreeSet::new());
}

#[test]
fn code_names_follow_the_section_3_2_spellings() {
    let named = |namespace: &'static str, code: &str| {
        code_name(&LossKind::namespaced(
            crate::report::loss::LossNamespace::new(namespace).expect("namespace"),
            code,
            LossTaxonomy::DecodeDiagnostic,
        ))
    };
    assert_eq!(named("export", "record.invalid"), "export:record.invalid");
    assert_eq!(named("export", "output.empty"), "export:output.empty");
    assert_eq!(named("measure", "output.empty"), "measure:output.empty");
    assert_eq!(
        named("decode", "depth.not-delivered"),
        "decode/depth.not-delivered"
    );
    assert_eq!(
        named("read", "spelling.normalized"),
        "read/spelling.normalized"
    );
    assert_eq!(named("decode", "record.unreadable"), "record.unreadable");
    assert_eq!(
        named("rhino", "target.source-dialect-displaced"),
        "target.source-dialect-displaced"
    );
    assert_eq!(
        named("f3d", "geometry.vertex-tolerance-widened"),
        "geometry.vertex-tolerance-widened"
    );
    assert_eq!(
        named("nx", "topology.face-boundary-unstated"),
        "topology.face-boundary-unstated"
    );
    assert_eq!(
        named("iges", "global.metadata-field-unusable"),
        "iges/global.metadata-field-unusable"
    );
    assert_eq!(
        code_name(&LossKind::shared(LossTaxonomy::PcurveOmitted)),
        "shared/pcurve_omitted"
    );
}
