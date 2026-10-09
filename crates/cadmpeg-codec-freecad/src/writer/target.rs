// SPDX-License-Identifier: Apache-2.0
//! Source dialect eligibility and retained document selection for `FCStd` writes.

use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    target::ResolvedWrite, Consumption, EncodeInput, ExportBody, PatchConsumption, WritePath,
};
use cadmpeg_ir::document::CadIr;

use super::write;
use crate::native::DocumentFacts;

/// Native document and source declaration selected for a retained-entry write.
///
/// The writer patches retained `Document.xml` and repacks the native entries.
/// [`retained_baseline`] selects the native namespace, sole document record
/// and source schema declaration. [`resolve`] first checks source dialect
/// eligibility. The byte writer validates the resulting property patch.
#[derive(Debug)]
pub(super) struct Resolution<'a> {
    ir: &'a CadIr,
    namespace: &'a cadmpeg_ir::native::NativeNamespace,
    document: DocumentFacts,
    schema_version: &'a str,
    target: DialectId,
}

impl<'a> Resolution<'a> {
    pub(super) const fn ir(&self) -> &'a CadIr {
        self.ir
    }

    pub(super) const fn namespace(&self) -> &'a cadmpeg_ir::native::NativeNamespace {
        self.namespace
    }

    pub(super) const fn document(&self) -> &DocumentFacts {
        &self.document
    }

    pub(super) fn schema_version(&self) -> &str {
        self.schema_version
    }

    pub(super) const fn target(&self) -> &DialectId {
        &self.target
    }
}

const BASELINE_UNAVAILABLE: &str =
    "the retained FCStd document graph is unavailable, and this writer regenerates no \
     Document.xml, so the target cannot be written";
const TRANSCODE_UNAVAILABLE: &str =
    "the resolved target displaces the retained FCStd source dialect, and this writer \
     regenerates no Document.xml, so the target cannot be written";

/// Plan the retained-entry export selected by source and target resolution.
///
/// [`resolve`] requires source-preservation eligibility. Native document
/// selection requires the `fcstd` namespace, one document record and a source
/// schema declaration. The selected schema controls written-property parsing.
/// The byte writer validates the native declarations and property graph before
/// it repacks the entries. The export reports independent patch consumption.
pub(crate) fn plan(
    input: EncodeInput<'_>,
    resolved: &ResolvedWrite<'_>,
) -> Result<ExportBody, CodecError> {
    let resolution = resolve(input.ir, resolved)?;
    finish(&resolution)
}

/// Write the resolved export. The sealed encoder stamps the target identity
/// and the fidelity resolution; this writer patches the retained document and
/// consumes no sidecar.
fn finish(resolution: &Resolution<'_>) -> Result<ExportBody, CodecError> {
    let mut bytes = Vec::new();
    let outcome = write(&mut bytes, resolution)?;
    Ok(ExportBody {
        bytes,
        census: outcome.census,
        write_path: WritePath::Patched {
            consumption: PatchConsumption::Independent(Consumption::NotConsumed),
        },
        losses: Vec::new(),
        notes: outcome.notes,
    })
}

/// Decide what to write, from the request and the source.
fn resolve<'a>(ir: &'a CadIr, resolved: &ResolvedWrite<'_>) -> Result<Resolution<'a>, CodecError> {
    if !resolved.source_preservation_eligible() {
        return Err(resolved.unavailable(TRANSCODE_UNAVAILABLE));
    }
    // Target resolution already classified the source declaration. The
    // accepted Resolution threads that witness to the byte writer; the byte
    // writer does not classify the native declaration again.
    retained_baseline(ir, resolved.target_id())
        .ok_or_else(|| resolved.unavailable(BASELINE_UNAVAILABLE))
}

/// Retained native document and source schema selected for a target.
///
/// Select the sole native document record and the source schema declaration.
/// `target` carries the identity selected by source dialect resolution.
pub(super) fn retained_baseline<'a>(ir: &'a CadIr, target: &DialectId) -> Option<Resolution<'a>> {
    let namespace = ir.native.namespace("fcstd")?;
    let [document]: [DocumentFacts; 1] = namespace
        .arena_as::<DocumentFacts>("document")
        .ok()?
        .try_into()
        .ok()?;
    let schema_version = ir
        .source
        .as_ref()?
        .dialect()?
        .declared()
        .get(crate::dialect::DECLARED_SCHEMA_VERSION)?
        .as_str();
    Some(Resolution {
        ir,
        namespace,
        document,
        schema_version,
        target: target.clone(),
    })
}
