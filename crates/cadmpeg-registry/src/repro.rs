// SPDX-License-Identifier: Apache-2.0
//! Embeddable reproduction API.
use crate::InputCatalog;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::report::{
    check::{Check, Finding},
    Severity,
};
use cadmpeg_ir::repro::{edges, record, retain, rows};
pub use cadmpeg_ir::repro::{
    Bundle, CutOptions, Document, ManifestEntry, NumberBand, Signature, Stage,
};
use cadmpeg_measure::WorkProfile;
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

/// Cut a finding with codec-owned native string classification.
pub fn cut(
    document: &Document,
    finding: &Finding,
    options: &CutOptions,
) -> Result<Bundle, cadmpeg_core::CodecError> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let bytes = cadmpeg_core::decode::u64_from_index(cadmpeg_ir::repro::projection_size(
        document, options,
    )?);
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, bytes)?;
    let fields = cadmpeg_ir::native::privacy::collect(100_000, || {
        crate::validate_native(&ctx, &InputCatalog::with_builtins(), document, None)
    })?;
    ctx.finish_session()?;
    let mut replacements = std::collections::BTreeMap::new();
    for descriptor in InputCatalog::with_builtins().descriptors() {
        if let Some(codec) = descriptor.codec() {
            replacements.extend(codec.repro_redactions(document)?);
        }
    }
    cadmpeg_ir::repro::cut_with_native(document, finding, options, &fields, &replacements)
}

/// Check a fragment with the same composed validators as the CLI.
pub fn check(bundle: &Bundle) -> Vec<Finding> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let result = (|| {
        let size = cadmpeg_ir::repro::projection_size(&bundle.document, &CutOptions::default())?;
        let ctx = DecodeContext::for_loaded_input(
            &arena,
            &policy,
            cadmpeg_core::decode::u64_from_index(size),
        )?;
        let report = crate::check_with_context(
            &ctx,
            &InputCatalog::with_builtins(),
            &bundle.document,
            None,
            Vec::new(),
            WorkProfile::Service,
        )?;
        ctx.finish_session()?;
        Ok::<_, cadmpeg_core::CodecError>(report.findings)
    })();
    result.unwrap_or_else(|error| {
        vec![Finding {
            check: Check::MeasurementIncomplete,
            severity: Severity::Blocking,
            entity: None,
            related: Vec::new(),
            message: format!("fragment validation stopped: {error}"),
        }]
    })
}

/// Reduce rows and their incoming dependents while retaining the signature.
pub fn shrink(
    bundle: &Bundle,
    signature: &Signature,
    budget: Duration,
    cancel: &dyn Fn() -> bool,
) -> Bundle {
    let start = Instant::now();
    let stopped = || start.elapsed() >= budget || cancel();
    let mut best = bundle.clone();
    if stopped() {
        return best;
    }
    if signature.stage != Stage::Check
        || !check(bundle)
            .iter()
            .any(|finding| signature.reproduces(finding))
        || stopped()
    {
        return best;
    }
    let mut width = usize::MAX;
    loop {
        if stopped() {
            break;
        }
        let Ok(value) = serde_json::to_value(&best.document) else {
            break;
        };
        let all = rows(&value);
        if all.is_empty() {
            break;
        }
        let graph = edges(&value, &all);
        width = width.min(all.len().div_ceil(2)).max(1);
        let mut improved = false;
        for begin in (0..all.len()).step_by(width) {
            if stopped() {
                return best;
            }
            let mut drop: BTreeSet<usize> = (begin..(begin + width).min(all.len())).collect();
            loop {
                let before = drop.len();
                let native_groups: BTreeSet<String> = drop
                    .iter()
                    .filter_map(|index| {
                        all[*index]
                            .arena
                            .strip_prefix("/native/")
                            .and_then(|path| path.split('/').next())
                            .map(str::to_owned)
                    })
                    .collect();
                for (index, row) in all.iter().enumerate() {
                    if row
                        .arena
                        .strip_prefix("/native/")
                        .and_then(|path| path.split('/').next())
                        .is_some_and(|codec| native_groups.contains(codec))
                    {
                        drop.insert(index);
                    }
                }

                for (index, targets) in graph.iter().enumerate() {
                    if targets.iter().any(|target| drop.contains(target)) {
                        drop.insert(index);
                    }
                }
                if before == drop.len() {
                    break;
                }
                if stopped() {
                    return best;
                }
            }
            let keep: BTreeSet<usize> = (0..all.len())
                .filter(|index| !drop.contains(index))
                .collect();
            let mut candidate_value = value.clone();
            retain(&mut candidate_value, &all, &keep);
            let Ok(document) = serde_json::from_value(candidate_value) else {
                continue;
            };
            let mut candidate = best.clone();
            candidate.document = document;
            candidate.roots.retain(|id| {
                keep.iter()
                    .any(|index| record(&value, &all[*index])["id"].as_str() == Some(id))
            });
            if stopped() {
                return best;
            }
            let reproduced = check(&candidate)
                .into_iter()
                .find(|finding| signature.reproduces(finding));
            if stopped() {
                return best;
            }
            if let Some(finding) = reproduced {
                candidate.roots = finding.entity.into_iter().chain(finding.related).collect();
                if candidate.refresh_manifest().is_err() {
                    return best;
                }
                best = candidate;
                improved = true;
                break;
            }
        }
        if !improved {
            if width == 1 {
                break;
            }
            width = width.div_ceil(2);
        }
    }
    best
}
