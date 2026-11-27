// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::indexing_slicing))]
//! Decode offline Onshape bundles into CADIR.
//!
//! <!-- generated: capability onshape -->
//! Support: L4 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#onshape-bundle)).
//! <!-- /generated: capability onshape -->
mod archive;
mod detect;
mod dialect;
mod error;
mod loss;
mod reader;

use cadmpeg_core::decode::{DecodeContext, View, WholeInput};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{CodecBackend, Confidence, Decoded, FormatId};
use cadmpeg_ir::{ContainerKind, ContainerSummary};

type StepDecoder = for<'a> fn(&DecodeContext<'a>, View<'a>) -> Result<Decoded, CodecError>;

/// Decoder for `dutchman.onshape-bundle/1` ZIP bundles.
#[derive(Clone, Copy)]
pub struct OnshapeCodec {
    step_decoder: StepDecoder,
}
impl OnshapeCodec {
    /// Construct with the registry's STEP backend. The callback shares the parent budget.
    pub const fn new(step_decoder: StepDecoder) -> Self {
        Self { step_decoder }
    }
}
impl CodecBackend for OnshapeCodec {
    const FORMAT: FormatId = FormatId::new(dialect::FORMAT);
    fn detect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        prefix: View<'_>,
    ) -> Result<Confidence, CodecError> {
        detect::prefix(ctx, prefix)
    }
    fn detect_whole_impl(
        &self,
        ctx: &DecodeContext<'_>,
        input: WholeInput<'_>,
    ) -> Result<Confidence, CodecError> {
        detect::whole(ctx, input.view())
    }
    fn inspect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        let bundle = archive::Bundle::open(ctx, root)?;
        dialect::validate(bundle.json("manifest.json")?)?;
        Ok(ContainerSummary::classified(
            dialect::layers(),
            ContainerKind::Zip,
            bundle
                .snapshot
                .container_entries(|_| cadmpeg_core::container::ContainerRole::Other),
            Vec::new(),
            Vec::new(),
        ))
    }
    fn decode_impl(&self, ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
        let mut report = reader::Report::default();
        let mut model = reader::read(ctx, root, self.step_decoder, &mut report)?;
        let mut layers = dialect::layers();
        if let Some(nested) = model.ir.source.as_ref().and_then(|s| s.dialects()) {
            for matched in nested.iter() {
                let instance = matched.instance().map_or_else(
                    || "export.step".to_owned(),
                    |inner| format!("export.step/{inner}"),
                );
                layers
                    .insert(matched.clone().with_instance(instance))
                    .map_err(|e| CodecError::malformed(e.to_string()))?;
            }
        }
        let attributes = model
            .ir
            .source
            .take()
            .map(|s| s.attributes)
            .unwrap_or_default();
        model.ir.source = Some(cadmpeg_ir::document::SourceMeta::classified(
            layers, attributes,
        ));
        model.ir.state_root_definition(
            ctx,
            cadmpeg_ir::ids::ProductDefinitionId::compose(
                &cadmpeg_ir::identity_namespace!("onshape", "model", "product"),
                cadmpeg_ir::identity_key!("root"),
            ),
            cadmpeg_ir::products::ExternalDocument::default(),
        )?;
        model.body.losses.extend(report.notes);
        model.source_fidelity.decode_digest =
            cadmpeg_ir::SourceFidelity::for_root(root).decode_digest;
        model.ir.finalize();
        Ok(model.into_decoded())
    }
}
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod test_support;
