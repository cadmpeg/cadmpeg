// SPDX-License-Identifier: Apache-2.0
use crate::repro::*;
use cadmpeg_ir::{math::Point3, report::check::Check, CadIr};
use std::time::Duration;

fn bundle() -> Bundle {
    let mut document = cadmpeg_ir::examples::unit_cube().unwrap();
    document.model.points[0].set_position(Point3::new(50.0, 0.0, 0.0).try_into().unwrap());
    let finding = cadmpeg_ir::validate_neutral(&document, Vec::new())
        .unwrap()
        .findings
        .into_iter()
        .find(|finding| finding.check == Check::GeometricConsistency)
        .unwrap();
    let mut bundle = cut(&document, &finding, &CutOptions::default()).unwrap();
    let unrelated = cadmpeg_ir::examples::unit_cube().unwrap();
    bundle
        .document
        .model
        .points
        .extend(unrelated.model.points.into_iter().map(|mut point| {
            point.id = cadmpeg_ir::ids::PointId::mint(format!(
                "synthetic:extra:point#{}",
                point.id.as_str().split('#').next_back().unwrap()
            ))
            .unwrap();
            point
        }));
    bundle.refresh_manifest().unwrap();
    bundle
}

#[test]
fn repro_composed_check_and_shrink_keep_signature_and_reduce_rows() {
    let bundle = bundle();
    assert!(check(&bundle)
        .iter()
        .any(|finding| bundle.signature.reproduces(finding)));
    let reduced = shrink(&bundle, &bundle.signature, Duration::from_secs(10), &|| {
        false
    });
    assert!(reduced.document.model.points.len() < bundle.document.model.points.len());
    assert!(check(&reduced)
        .iter()
        .any(|finding| bundle.signature.reproduces(finding)));
    assert!(check(&reduced)
        .iter()
        .all(|finding| finding.check != Check::ReferentialIntegrity));
    let parsed: CadIr =
        serde_json::from_value(serde_json::to_value(&reduced.document).unwrap()).unwrap();
    assert!(parsed.fragment);
}

#[test]
fn repro_shrink_budget_and_cancel_return_last_bundle() {
    let bundle = bundle();
    assert_eq!(
        shrink(&bundle, &bundle.signature, Duration::ZERO, &|| false),
        bundle
    );
    assert_eq!(
        shrink(&bundle, &bundle.signature, Duration::from_secs(10), &|| {
            true
        }),
        bundle
    );
    let calls = std::cell::Cell::new(0);
    let reduced = shrink(&bundle, &bundle.signature, Duration::from_secs(10), &|| {
        calls.set(calls.get() + 1);
        calls.get() > 3
    });
    assert!(calls.get() > 3);
    assert!(check(&reduced)
        .iter()
        .any(|finding| bundle.signature.reproduces(finding)));
}

#[test]
fn repro_shrink_elapsed_budget_stops_search() {
    let bundle = bundle();
    let start = std::time::Instant::now();
    let reduced = shrink(
        &bundle,
        &bundle.signature,
        Duration::from_millis(1),
        &|| false,
    );
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(check(&reduced)
        .iter()
        .any(|finding| bundle.signature.reproduces(finding)));
}

#[test]
fn repro_nonreproducing_cut_retains_rows_and_reason() {
    let document = cadmpeg_ir::examples::unit_cube().unwrap();
    let finding = cadmpeg_ir::report::check::Finding {
        check: Check::GeometricConsistency,
        severity: cadmpeg_ir::report::Severity::Error,
        entity: Some(document.model.bodies[0].id.to_string()),
        related: Vec::new(),
        message: "synthetic finding requiring additional context".into(),
    };
    let bundle = cut(&document, &finding, &CutOptions::default()).unwrap();
    assert!(bundle.incomplete_reason.is_some());
    assert_eq!(bundle.document.model.bodies.len(), 1);
    assert_eq!(
        bundle.document.model.faces.len(),
        document.model.faces.len()
    );
    let parsed: Bundle = serde_json::from_slice(&serde_json::to_vec(&bundle).unwrap()).unwrap();
    assert_eq!(parsed, bundle);
    assert!(check(&bundle)
        .iter()
        .any(|finding| finding.check == Check::FragmentSkipped));
}

#[test]
fn repro_body_orientation_uses_the_composed_document_check() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut document = cadmpeg_ir::examples::unit_cube().unwrap();
    document.model.faces[0].sense = match document.model.faces[0].sense {
        cadmpeg_ir::topology::Sense::Forward => cadmpeg_ir::topology::Sense::Reversed,
        cadmpeg_ir::topology::Sense::Reversed => cadmpeg_ir::topology::Sense::Forward,
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
    let report = crate::check_with_context(
        &ctx,
        &crate::InputCatalog::with_builtins(),
        &document,
        None,
        Vec::new(),
        cadmpeg_measure::WorkProfile::Desktop,
    )
    .unwrap();
    let finding = report
        .findings
        .iter()
        .find(|finding| {
            finding.check == Check::Orientation
                && finding.entity.as_deref() == Some(document.model.bodies[0].id.as_str())
        })
        .unwrap();
    let bundle = cut(&document, finding, &CutOptions::default()).unwrap();
    assert!(
        bundle.incomplete_reason.is_none(),
        "{:?}",
        bundle.incomplete_reason
    );
    assert_eq!(
        bundle.document.model.faces.len(),
        document.model.faces.len()
    );
    assert!(check(&bundle)
        .iter()
        .any(|finding| bundle.signature.reproduces(finding)));
}
