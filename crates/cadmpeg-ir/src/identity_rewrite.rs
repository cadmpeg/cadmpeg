// SPDX-License-Identifier: Apache-2.0
//! Direct field implementations for the typed identity rewrite contract.

macro_rules! rewrite_scalar {
    ($type:ty) => {
        impl crate::schema::rewrite::typed::RewriteIdentities for $type {
            fn rewrite_native_value<
                RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>,
            >(
                ctx: &cadmpeg_core::decode::DecodeContext<'_>,
                _value: &mut serde_json::Value,
                _map: &mut crate::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>,
            ) -> Result<(), cadmpeg_core::CodecError> {
                ctx.charge_work(1, "walk native identity scalar")
            }
            fn visit_identity_references(
                &self,
                ctx: &cadmpeg_core::decode::DecodeContext<'_>,
                _visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>,
            ) -> Result<(), cadmpeg_core::CodecError> {
                ctx.charge_work(1, "walk typed reference scalar")
            }
            fn rewrite_identities<RewriteMapFn>(
                self,
                rewrite_context: &cadmpeg_core::decode::DecodeContext<'_>,
                _identity_map: &mut crate::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>,
            ) -> Result<Self, cadmpeg_core::CodecError>
            where
                RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>,
            {
                rewrite_context.charge_work(1, "typed rewrite value")?;
                Ok(self)
            }
        }
    };
}

// Full-fidelity references use string wire fields, but their owner declares
// identity semantics. Ordinary display/source text remains ordinary text.
macro_rules! rewrite_record_field {
    (native_ref, $value:expr, $ctx:expr, $map:expr) => {
        crate::schema::rewrite::typed::native_fields::FullFidelityReference::rewrite(
            $value, $ctx, $map,
        )
    };
    (geometry_ref, $value:expr, $ctx:expr, $map:expr) => {
        crate::schema::rewrite::typed::native_fields::FullFidelityReference::rewrite(
            $value, $ctx, $map,
        )
    };
    (endpoint_refs, $value:expr, $ctx:expr, $map:expr) => {
        crate::schema::rewrite::typed::native_fields::FullFidelityReference::rewrite(
            $value, $ctx, $map,
        )
    };
    ($field:ident, $value:expr, $ctx:expr, $map:expr) => {
        crate::schema::rewrite::typed::RewriteIdentities::rewrite_identities($value, $ctx, $map)
    };
}

macro_rules! rewrite_native_record_field {
    (native_ref, $ctx:expr, $value:expr, $map:expr, $owner:ty) => {
        crate::schema::rewrite::typed::native_fields::rewrite_reference(
            $ctx,
            $value,
            "native_ref",
            $map,
        )
    };
    (geometry_ref, $ctx:expr, $value:expr, $map:expr, $owner:ty) => {
        crate::schema::rewrite::typed::native_fields::rewrite_reference(
            $ctx,
            $value,
            "geometry_ref",
            $map,
        )
    };
    (endpoint_refs, $ctx:expr, $value:expr, $map:expr, $owner:ty) => {
        crate::schema::rewrite::typed::native_fields::rewrite_reference(
            $ctx,
            $value,
            "endpoint_refs",
            $map,
        )
    };
    ($field:ident, $ctx:expr, $value:expr, $map:expr, $owner:ty) => {
        crate::schema::rewrite::typed::native_fields::rewrite_field(
            $ctx,
            $value,
            stringify!($field),
            $map,
            |owner: &$owner| Some(&owner.$field),
        )
    };
}

macro_rules! rewrite_record {
    ($type:ty, [$($generic:ident $(: $bound:path)?),*]; {$($field:ident),* $(,)?}) => {
        impl<$($generic: crate::schema::rewrite::typed::RewriteIdentities $(+ $bound)?),*> crate::schema::rewrite::typed::RewriteIdentities for $type {
            fn rewrite_native_value<RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(ctx: &cadmpeg_core::decode::DecodeContext<'_>, value: &mut serde_json::Value, map: &mut crate::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>) -> Result<(), cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("walk native typed fields")?;
                ctx.charge_work(1, "walk native typed fields")?;
                $(rewrite_native_record_field!($field, ctx, value, map, Self)?;)*
                Ok(())
            }
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
                Ok(Self { $($field: rewrite_record_field!($field, $field, rewrite_context, identity_map)?),* })
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
