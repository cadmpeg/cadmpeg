// SPDX-License-Identifier: Apache-2.0
//! Source preservation and catalog synthesis have distinct actual targets.

use super::{catalog_write_ir, resolve_write_request, TargetRequest, CATALOG_WRITE_CATALOG};
use crate::codec::write::EncodeError;
use crate::codec::write::{
    target::{Catalog, ResolvedTarget, ResolvedWrite},
    ArenaCoverage, EncodeInput, Encoder, EncoderBackend, ExportBody, PatchConsumption, WritePath,
};
use crate::codec::FormatId;
use crate::source_fidelity::SourceFidelity;
use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_core::target::{DefaultSource, TargetCatalog};

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
fn off_catalog_inherit_selects_the_default_and_names_the_displacement() {
    let ir = catalog_write_ir(Some(("test", Some("test:future"))));
    let actual = resolve_write_request(&ir, TargetRequest::Inherit, CATALOG_WRITE_CATALOG)
        .expect("the catalog has a default");
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
    assert_eq!(actual.for_synthesis().unwrap().index(), 1);
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
    let EncodeError::UnsupportedTarget(refusal) = error else {
        panic!("only target resolution refuses")
    };
    assert_eq!(
        serde_json::to_value(refusal).expect("typed refusal")["refusal"],
        serde_json::json!({"kind":"no_default","source":{"kind":"unrecorded_dialect"}})
    );
    let ir = catalog_write_ir(Some(("test", Some("test:future"))));
    let error = resolve_write_request(&ir, TargetRequest::Inherit, catalog)
        .expect_err("inheritance requires the missing default");
    let EncodeError::UnsupportedTarget(refusal) = error else {
        panic!("only target resolution refuses")
    };
    assert_eq!(
        serde_json::to_value(refusal).expect("typed refusal")["refusal"],
        serde_json::json!({"kind":"no_default","source":{"kind":"off_catalog_dialect","dialect":"test:future"}})
    );
}

struct PathProbe<const PATH: u8>;

impl<const PATH: u8> EncoderBackend for PathProbe<PATH> {
    const FORMAT: FormatId = FormatId::new("test");
    const REQUEST: DecodeDepth = DecodeDepth::Full;
    const REUSES_SOURCE: bool = true;
    type Target = Catalog;
    const TARGET: Catalog = Catalog::new(super::CATALOG_WRITE_TARGETS, Some(1));

    fn plan_resolved(
        &self,
        input: EncodeInput<'_>,
        target: ResolvedWrite<'_>,
    ) -> Result<ExportBody, EncodeError> {
        assert_eq!(target.target_id().as_str(), "test:old");
        let mut body = ExportBody::synthesized(Vec::new(), input.ir, ArenaCoverage::Complete);
        if PATH == 0 {
            body.write_path = WritePath::VerbatimReplay;
        } else if PATH == 1 {
            body.write_path = WritePath::Patched {
                consumption: PatchConsumption::Replayed,
            };
        } else {
            let actual = target.for_synthesis()?;
            assert_eq!(actual.resolution().target_id().as_str(), "test:old");
        }
        Ok(body)
    }
}

#[test]
fn sealed_report_names_the_actual_preserved_or_synthesized_target() {
    let ir = catalog_write_ir(Some(("test", Some("test:old"))));
    let fidelity = SourceFidelity::with_annotations(
        crate::Annotations::default(),
        crate::hash::digest::Sha256Digest::digest(b"test source"),
    );
    let input = EncodeInput::new(&ir, Some(&fidelity));
    let replay = PathProbe::<0>
        .plan(input, TargetRequest::Inherit)
        .expect("replay");
    let patch = PathProbe::<1>
        .plan(input, TargetRequest::Inherit)
        .expect("patch");
    let synthesis = PathProbe::<2>
        .plan(input, TargetRequest::Inherit)
        .expect("synthesis fallback");
    for (plan, expected) in [
        (replay, "test:old"),
        (patch, "test:old"),
        (synthesis, "test:old"),
    ] {
        let report = serde_json::to_value(plan.report()).expect("export report");
        assert_eq!(report["identity"]["target"], expected);
    }
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
    let EncodeError::UnsupportedTarget(refusal) = error else {
        panic!("target token resolution")
    };
    assert_eq!(
        serde_json::to_value(refusal).expect("typed refusal")["refusal"],
        serde_json::json!({"kind":"unknown_explicit","requested":"future"})
    );
}
