// SPDX-License-Identifier: Apache-2.0
//! Registry dispatch for bounded feature-only scans.

use cadmpeg_container::compound::scan::ScanCompound;
use cadmpeg_container::scan::ScanZip;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy, View};
use cadmpeg_core::scan::{DesignScan, ScanBudget, ScanClock, ScanLimits, ScanReader, StopReason};

use crate::{DetectionOutcome, InputCatalog};

const DETECTION_PREFIX_BYTES: usize = 4096;
const DETECTION_STORAGE_BYTES: u64 = 8192;

/// Detects and scans a native feature listing under one shared budget.
pub fn scan_design(input: View<'_>, limits: ScanLimits, cancel: &dyn ScanClock) -> DesignScan {
    InputCatalog::with_builtins().scan_design(input, limits, cancel)
}

impl InputCatalog {
    /// Detects a reader and scans its listing while preserving a bounded prefix.
    pub fn scan_design(
        &self,
        input: View<'_>,
        limits: ScanLimits,
        cancel: &dyn ScanClock,
    ) -> DesignScan {
        let budget = ScanBudget::new(limits, cancel);
        let mut scan = DesignScan::new(None);
        let result = detect(self, input, &budget);
        match result {
            Ok(id) => {
                scan.format = Some(id.clone());
                if let Some(descriptor) = self
                    .descriptors()
                    .find(|descriptor| descriptor.format_id().as_str() == id)
                {
                    return descriptor.scan(input, &budget);
                }
                scan.finish(
                    &budget,
                    Err(StopReason::Unsupported(format!(
                        "reader {id} is absent from this build"
                    ))),
                );
            }
            Err(reason) => scan.finish(&budget, Err(reason)),
        }
        scan
    }
}

fn detect(
    catalog: &InputCatalog,
    input: View<'_>,
    budget: &ScanBudget<'_>,
) -> Result<String, StopReason> {
    budget.check()?;
    budget.retain(DETECTION_STORAGE_BYTES)?;
    let mut reader = ScanReader::source(input, budget);
    let head = reader.take(input.window().len().min(8))?;
    if head
        .window()
        .starts_with(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1])
    {
        let container = ScanCompound::new(input, budget)?;
        let mut selected = None;
        for descriptor in catalog.descriptors() {
            budget.check()?;
            if descriptor.scan_compound_match(container.entries(), budget)? {
                if selected.is_some() {
                    return Err(StopReason::Unrecognized);
                }
                selected = Some(descriptor.format_id().as_str().to_owned());
            }
        }
        return selected.ok_or_else(|| {
            StopReason::Unsupported("compound reader has no bounded feature scan".into())
        });
    }
    if head.window().starts_with(b"PK\x03\x04") || head.window().starts_with(b"PK\x05\x06") {
        return detect_zip(catalog, input, budget);
    }
    let length = input.window().len().min(DETECTION_PREFIX_BYTES);
    reader.take(length - head.window().len())?;
    let end = input
        .start()
        .checked_add(length)
        .ok_or(StopReason::SourceBytes)?;
    let prefix = input
        .child(input.start(), end)
        .ok_or_else(|| StopReason::Truncated("detection prefix".into()))?;
    if cadmpeg_container::ArchiveSnapshot::has_footer(prefix.window()) {
        return detect_zip(catalog, input, budget);
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = DETECTION_STORAGE_BYTES;
    policy.limits.max_decompressed_bytes_total = 0;
    policy.limits.max_decompressed_bytes_per_expand = 0;
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Container);
    let outcome = catalog
        .detect(&ctx, prefix)
        .map_err(|error| StopReason::Malformed(error.to_string()))?;
    budget.check()?;
    match outcome {
        DetectionOutcome::Detected { codec, .. } => Ok(codec.id().as_str().to_owned()),
        DetectionOutcome::None if crate::catalog::is_cadir_prefix(prefix.window()) => {
            Ok(crate::Format::Cadir.name().as_str().to_owned())
        }
        DetectionOutcome::None | DetectionOutcome::Ambiguous(_) => Err(StopReason::Unrecognized),
    }
}

fn detect_zip(
    catalog: &InputCatalog,
    input: View<'_>,
    budget: &ScanBudget<'_>,
) -> Result<String, StopReason> {
    let mut archive = ScanZip::new(input, budget, true)?;
    let entries = archive.entries()?;
    let mut selected = None;
    for descriptor in catalog.descriptors() {
        let mut matched = false;
        for entry in &entries {
            budget.check()?;
            matched |= descriptor.scan_zip_match(&entry.name);
        }
        if matched {
            if selected.is_some() {
                return Err(StopReason::Unrecognized);
            }
            selected = Some(descriptor.format_id().as_str().to_owned());
        }
    }
    selected.ok_or(StopReason::Unrecognized)
}
