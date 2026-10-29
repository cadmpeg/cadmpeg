// SPDX-License-Identifier: Apache-2.0
//! Source preservation and catalog synthesis have distinct actual targets.

use super::{catalog_write_ir, resolve_write_request, TargetRequest, CATALOG_WRITE_CATALOG};
use crate::codec::write::{
    target::{Catalog, ResolvedTarget, ResolvedWrite},
    ArenaCoverage, EncodeInput, Encoder, EncoderBackend, ExportBody, PatchConsumption, WritePath,
};
use crate::codec::FormatId;
use crate::source_fidelity::SourceFidelity;
use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_core::target::{DefaultSource, TargetCatalog};
use cadmpeg_core::CodecError;

#[test]
fn unrecorded_inherit_selects_the_catalog_default_and_names_the_actual_displacement() {
    let ir = catalog_write_ir(Some(("test", None)));
    let resolved = resolve_write_request(&ir, TargetRequest::Inherit, CATALOG_WRITE_CATALOG)
        .expect("an unrecorded dialect selects the declared default");
    assert_eq!(resolved.target_id().as_str(), "test:new");
    assert!(matches!(
        resolved.target(),
        ResolvedTarget::Default {
            source: DefaultSource::UnrecordedDialect,
            ..
        }
    ));
    assert!(resolved.has_same_format_source());
    assert!(!resolved.source_preservation_eligible());
    assert_eq!(resolved.displaced_source(), None);
    assert_eq!(resolved.displacement_message().as_deref(), Some("the unrecorded same-format source dialect was displaced by target dialect test:new; the source dialect identity is not preserved"));
}

#[test]
fn off_catalog_preservation_keeps_its_source_until_synthesis_is_selected() {
    let ir = catalog_write_ir(Some(("test", Some("test:future"))));
    let preserved = resolve_write_request(&ir, TargetRequest::Inherit, CATALOG_WRITE_CATALOG)
        .expect("a recorded source is eligible for preservation");
    assert_eq!(preserved.target_id().as_str(), "test:future");
    assert!(preserved.preserves_source());
    let synthesis = preserved
        .for_synthesis()
        .expect("the catalog has a default");
    assert_eq!(synthesis.index(), 1);
    let actual = synthesis.resolution();
    assert_eq!(actual.target_id().as_str(), "test:new");
    assert!(
        matches!(actual.target(), ResolvedTarget::Default { source: DefaultSource::OffCatalogDialect(source), .. } if source.as_str() == "test:future")
    );
    assert!(actual.has_same_format_source());
    assert!(!actual.preserves_source());
    assert!(!actual.source_preservation_eligible());
    assert_eq!(
        actual
            .displaced_source()
            .map(cadmpeg_core::dialect::DialectId::as_str),
        Some("test:future")
    );
    assert_eq!(actual.displacement_message().as_deref(), Some("source dialect test:future was displaced by target dialect test:new; the source dialect identity is not preserved"));
    assert_eq!(preserved.target_id().as_str(), "test:future");
    assert!(preserved.preserves_source());
}

#[test]
fn explicit_catalog_selection_does_not_become_an_inherited_default() {
    let ir = catalog_write_ir(Some(("test", Some("test:future"))));
    let resolved =
        resolve_write_request(&ir, TargetRequest::Explicit("old"), CATALOG_WRITE_CATALOG)
            .expect("the explicit token names a catalog row");
    let synthesis = resolved.for_synthesis().expect("explicit synthesis row");
    assert_eq!(synthesis.index(), 0);
    assert!(matches!(
        synthesis.resolution().target(),
        ResolvedTarget::Explicit {
            requested: "old",
            ..
        }
    ));
    assert_eq!(synthesis.resolution().target_id().as_str(), "test:old");
}

#[test]
fn missing_default_refusal_preserves_the_same_format_source_relation() {
    let catalog = TargetCatalog::new(super::CATALOG_WRITE_TARGETS, None);
    let unrecorded = catalog_write_ir(Some(("test", None)));
    let error = resolve_write_request(&unrecorded, TargetRequest::Inherit, catalog)
        .expect_err("no catalog default exists");
    let CodecError::UnsupportedTarget(refusal) = error else {
        panic!("only target resolution refuses")
    };
    assert_eq!(
        serde_json::to_value(refusal).expect("typed refusal")["refusal"],
        serde_json::json!({"kind":"no_default","source":{"kind":"unrecorded_dialect"}})
    );
    let ir = catalog_write_ir(Some(("test", Some("test:future"))));
    let preserved = resolve_write_request(&ir, TargetRequest::Inherit, catalog)
        .expect("exact preservation does not need a synthesis default");
    let error = preserved
        .for_synthesis()
        .expect_err("fallback requires the missing default");
    let CodecError::UnsupportedTarget(refusal) = error else {
        panic!("only target resolution refuses")
    };
    assert_eq!(
        serde_json::to_value(refusal).expect("typed refusal")["refusal"],
        serde_json::json!({"kind":"no_default","source":{"kind":"off_catalog_dialect","dialect":"test:future"}})
    );
}

struct PathProbe<const PATH: u8, const DEFAULT: bool>;

impl<const PATH: u8, const DEFAULT: bool> EncoderBackend for PathProbe<PATH, DEFAULT> {
    const FORMAT: FormatId = FormatId::new("test");
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const REUSES_SOURCE: bool = true;
    type Target = Catalog;
    const TARGET: Catalog = Catalog::new(
        super::CATALOG_WRITE_TARGETS,
        if DEFAULT { Some(1) } else { None },
    );

    fn plan_resolved(
        &self,
        input: EncodeInput<'_>,
        target: ResolvedWrite<'_>,
    ) -> Result<ExportBody, CodecError> {
        assert_eq!(target.target_id().as_str(), "test:future");
        let mut body = ExportBody::synthesized(Vec::new(), input.ir, ArenaCoverage::Complete);
        if PATH == 0 {
            body.write_path = WritePath::VerbatimReplay;
        } else if PATH == 1 {
            body.write_path = WritePath::Patched {
                consumption: PatchConsumption::Replayed,
            };
        } else {
            let actual = target.for_synthesis()?;
            assert_eq!(actual.resolution().target_id().as_str(), "test:new");
        }
        Ok(body)
    }
}

#[test]
fn sealed_report_names_the_actual_preserved_or_synthesized_target() {
    let ir = catalog_write_ir(Some(("test", Some("test:future"))));
    let fidelity = SourceFidelity::default();
    let input = EncodeInput::new(&ir, Some(&fidelity));
    let replay = PathProbe::<0, true>
        .plan(input, TargetRequest::Inherit)
        .expect("replay");
    let patch = PathProbe::<1, true>
        .plan(input, TargetRequest::Inherit)
        .expect("patch");
    let synthesis = PathProbe::<2, true>
        .plan(input, TargetRequest::Inherit)
        .expect("synthesis fallback");
    for (plan, expected) in [
        (replay, "test:future"),
        (patch, "test:future"),
        (synthesis, "test:new"),
    ] {
        let report = serde_json::to_value(plan.report()).expect("export report");
        assert_eq!(report["identity"]["target"], expected);
    }
}

#[test]
fn exact_replay_without_a_default_remains_eligible_and_synthesis_refuses_only_no_default() {
    let ir = catalog_write_ir(Some(("test", Some("test:future"))));
    let fidelity = SourceFidelity::default();
    let input = EncodeInput::new(&ir, Some(&fidelity));
    let replay = PathProbe::<0, false>
        .plan(input, TargetRequest::Inherit)
        .expect("actual replay needs no default");
    assert_eq!(
        serde_json::to_value(replay.report()).expect("report")["identity"]["target"],
        "test:future"
    );
    let error = PathProbe::<2, false>
        .plan(input, TargetRequest::Inherit)
        .expect_err("no synthesis default");
    let CodecError::UnsupportedTarget(refusal) = error else {
        panic!("only catalog resolution refuses")
    };
    assert_eq!(
        serde_json::to_value(refusal).expect("typed refusal")["refusal"]["kind"],
        "no_default"
    );
}

#[test]
fn the_original_schema_four_catalog_token_needs_no_input_condition_or_default() {
    const ROWS: &[cadmpeg_core::target::TargetDescriptor] =
        &[cadmpeg_core::target::TargetDescriptor {
            id: cadmpeg_core::dialect_id!("fcstd:schema-4"),
            aliases: &["4"],
        }];
    let ir = crate::CadIr::empty();
    let resolved = resolve_write_request(
        &ir,
        TargetRequest::Explicit("4"),
        TargetCatalog::new(ROWS, None),
    )
    .expect("a declared catalog row is always selected");
    let synthesis = resolved
        .for_synthesis()
        .expect("an explicit row needs no default");
    assert_eq!(synthesis.index(), 0);
    assert_eq!(
        synthesis.resolution().target_id().as_str(),
        "fcstd:schema-4"
    );
    assert!(matches!(
        synthesis.resolution().target(),
        ResolvedTarget::Explicit { requested: "4", .. }
    ));
    let error = resolve_write_request(
        &ir,
        TargetRequest::Explicit("future"),
        TargetCatalog::new(ROWS, None),
    )
    .expect_err("unknown explicit tokens still refuse");
    let CodecError::UnsupportedTarget(refusal) = error else {
        panic!("target token resolution")
    };
    assert_eq!(
        serde_json::to_value(refusal).expect("typed refusal")["refusal"],
        serde_json::json!({"kind":"unknown_explicit","requested":"future"})
    );
}
