// SPDX-License-Identifier: Apache-2.0
//! Direct field implementations for the typed identity rewrite contract.

macro_rules! rewrite_scalar {
    ($type:ty) => {
        impl crate::schema::rewrite::typed::RewriteIdentities for $type {
            fn visit_identity_references(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, _visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>) -> Result<(), cadmpeg_core::CodecError> {
                ctx.charge_work(1, "walk typed reference scalar")
            }
            fn rewrite_identities<RewriteMapFn>(self, rewrite_context: &cadmpeg_core::decode::DecodeContext<'_>, _identity_map: &mut crate::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>) -> Result<Self, cadmpeg_core::CodecError>
            where RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError> {
                rewrite_context.charge_work(1, "typed rewrite value")?;
                Ok(self)
            }
        }
    };
}

macro_rules! rewrite_record {
    ($type:ty, [$($generic:ident $(: $bound:path)?),*]; {$($field:ident),* $(,)?}) => {
        impl<$($generic: crate::schema::rewrite::typed::RewriteIdentities $(+ $bound)?),*> crate::schema::rewrite::typed::RewriteIdentities for $type {
            fn visit_identity_references(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>) -> Result<(), cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("walk typed reference fields")?;
                ctx.charge_work(1, "walk typed reference fields")?;
                $(crate::schema::rewrite::typed::RewriteIdentities::visit_identity_references(&self.$field, ctx, visitor)?;)*
                Ok(())
            }
            fn rewrite_identities<RewriteMapFn>(self, rewrite_context: &cadmpeg_core::decode::DecodeContext<'_>, identity_map: &mut crate::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>) -> Result<Self, cadmpeg_core::CodecError>
            where RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError> {
                let _depth = rewrite_context.enter_nested("typed rewrite record")?;
                rewrite_context.charge_work(1, "typed rewrite record")?;
                let Self { $($field),* } = self;
                Ok(Self { $($field: crate::schema::rewrite::typed::RewriteIdentities::rewrite_identities($field, rewrite_context, identity_map)?),* })
            }
        }
    };
    ($type:ty, [$($generic:ident $(: $bound:path)?),*]; ($($field:ident),* $(,)?)) => {
        impl<$($generic: crate::schema::rewrite::typed::RewriteIdentities $(+ $bound)?),*> crate::schema::rewrite::typed::RewriteIdentities for $type {
            fn visit_identity_references(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>) -> Result<(), cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("walk typed reference fields")?;
                ctx.charge_work(1, "walk typed reference fields")?;
                let Self($($field),*) = self;
                $(crate::schema::rewrite::typed::RewriteIdentities::visit_identity_references($field, ctx, visitor)?;)*
                Ok(())
            }
            fn rewrite_identities<RewriteMapFn>(self, rewrite_context: &cadmpeg_core::decode::DecodeContext<'_>, identity_map: &mut crate::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>) -> Result<Self, cadmpeg_core::CodecError>
            where RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError> {
                let _depth = rewrite_context.enter_nested("typed rewrite tuple")?;
                rewrite_context.charge_work(1, "typed rewrite tuple")?;
                let Self($($field),*) = self;
                Ok(Self($(crate::schema::rewrite::typed::RewriteIdentities::rewrite_identities($field, rewrite_context, identity_map)?),*))
            }
        }
    };
}

macro_rules! rewrite_enum {
    ($type:ty, []; {$($variant:ident),* $(,)?}) => {
        rewrite_scalar!($type);
    };
    ($type:ty, [$($generic:ident $(: $bound:path)?),*]; {
        $($variant:ident $(($($tuple:ident),*))? $({$($field:ident),*})?),* $(,)?
    }) => {
        impl<$($generic: crate::schema::rewrite::typed::RewriteIdentities $(+ $bound)?),*> crate::schema::rewrite::typed::RewriteIdentities for $type {
            fn visit_identity_references(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>) -> Result<(), cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("walk typed reference fields")?;
                ctx.charge_work(1, "walk typed reference fields")?;
                match self {
                    $(Self::$variant $(($($tuple),*))? $({$($field),*})? => {
                        $($(crate::schema::rewrite::typed::RewriteIdentities::visit_identity_references($tuple, ctx, visitor)?;)*)?
                        $($(crate::schema::rewrite::typed::RewriteIdentities::visit_identity_references($field, ctx, visitor)?;)*)?
                        Ok(())
                    },)*
                }
            }
            fn rewrite_identities<RewriteMapFn>(self, rewrite_context: &cadmpeg_core::decode::DecodeContext<'_>, identity_map: &mut crate::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>) -> Result<Self, cadmpeg_core::CodecError>
            where RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError> {
                let _depth = rewrite_context.enter_nested("typed rewrite variant")?;
                rewrite_context.charge_work(1, "typed rewrite variant")?;
                match self {
                    $(Self::$variant $(($($tuple),*))? $({$($field),*})? => Ok(Self::$variant
                        $(($(crate::schema::rewrite::typed::RewriteIdentities::rewrite_identities($tuple, rewrite_context, identity_map)?),*))?
                        $({$($field: crate::schema::rewrite::typed::RewriteIdentities::rewrite_identities($field, rewrite_context, identity_map)?),*})?
                    ),)*
                }
            }
        }
    };
}
