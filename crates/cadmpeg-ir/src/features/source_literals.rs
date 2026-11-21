use super::{Feature, FeatureOperation, FeatureSourceContent, UnresolvedFamily};
use crate::schema::rewrite::typed::native_fields::FullFidelityReference;
use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

impl RewriteIdentities for Feature {
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("feature references")?;
        self.id.visit_identity_references(ctx, visitor)?;
        self.product_definition
            .visit_identity_references(ctx, visitor)?;
        self.dependencies.visit_identity_references(ctx, visitor)?;
        self.source_content
            .visit_identity_references(ctx, visitor)?;
        self.evaluation.visit_identity_references(ctx, visitor)
    }

    fn visit_local_members(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        self.evaluation.visit_local_members(ctx, visitor)
    }

    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("feature identities")?;
        let preserve = matches!(
            self.evaluation.definition().operation(),
            FeatureOperation::DatumAxis { .. }
                | FeatureOperation::Unresolved {
                    family: UnresolvedFamily::DatumAxis
                }
        );
        let Self {
            id,
            product_definition,
            ordinal,
            name,
            suppressed,
            dependencies,
            source_properties,
            source_tag,
            source_text,
            mut source_content,
            evaluation,
            native_ref,
        } = self;
        let (name, source_properties, source_tag, source_text, source_content) = if preserve {
            source_content.0 = source_content
                .0
                .into_iter()
                .map(|content| match content {
                    literal @ FeatureSourceContent::Text(_) => Ok(literal),
                    typed => typed.rewrite_identities(ctx, map),
                })
                .collect::<Result<_, _>>()?;
            (
                name,
                source_properties,
                source_tag,
                source_text,
                source_content,
            )
        } else {
            (
                name.rewrite_identities(ctx, map)?,
                source_properties.rewrite_identities(ctx, map)?,
                source_tag.rewrite_identities(ctx, map)?,
                source_text.rewrite_identities(ctx, map)?,
                source_content.rewrite_identities(ctx, map)?,
            )
        };
        Ok(Self {
            id: id.rewrite_identities(ctx, map)?,
            product_definition: product_definition.rewrite_identities(ctx, map)?,
            ordinal,
            name,
            suppressed,
            dependencies: dependencies.rewrite_identities(ctx, map)?,
            source_properties,
            source_tag,
            source_text,
            source_content,
            evaluation: evaluation.rewrite_identities(ctx, map)?,
            native_ref: native_ref.rewrite(map)?,
        })
    }
}
