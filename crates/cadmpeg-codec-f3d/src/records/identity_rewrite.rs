// SPDX-License-Identifier: Apache-2.0
//! Field walks shared by the native record owners.

macro_rules! rewrite_native_scalar {
    ($type:ty) => {
        impl cadmpeg_ir::schema::rewrite::typed::RewriteIdentities for $type {
            fn rewrite_native_value<RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(ctx: &cadmpeg_core::decode::DecodeContext<'_>, _value: &mut serde_json::Value, _map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>) -> Result<(), cadmpeg_core::CodecError> {
                ctx.charge_work(1, "walk native identity scalar")
            }
            fn visit_identity_references(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, _visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>) -> Result<(), cadmpeg_core::CodecError> {
                ctx.charge_work(1, "walk typed reference scalar")
            }
            fn rewrite_identities<RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, _map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>) -> Result<Self, cadmpeg_core::CodecError> {
                ctx.charge_work(1, "rewrite native typed node")?;
                Ok(self)
            }
        }
    };
}

macro_rules! rewrite_native_record {
    ($type:ty, [$($generic:ident),*]; {$($field:ident),* $(,)?} $(; native $native:expr)?) => {
        impl<$($generic: cadmpeg_ir::schema::rewrite::typed::RewriteIdentities),*> cadmpeg_ir::schema::rewrite::typed::RewriteIdentities for $type {
            fn rewrite_native_value<RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(ctx: &cadmpeg_core::decode::DecodeContext<'_>, value: &mut serde_json::Value, map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>) -> Result<(), cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("walk native typed fields")?;
                ctx.charge_work(1, "walk native typed fields")?;
                rewrite_native_wire!(ctx, value, map, Self, {$($field),*}, [$($native)?])
            }
            fn visit_identity_references(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>) -> Result<(), cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("walk native typed references")?;
                ctx.charge_work(1, "walk native typed references")?;
                $(cadmpeg_ir::schema::rewrite::typed::RewriteIdentities::visit_identity_references(&self.$field, ctx, visitor)?;)*
                Ok(())
            }
            fn rewrite_identities<RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(mut self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>) -> Result<Self, cadmpeg_core::CodecError> {
                ctx.charge_work(1, "rewrite native typed node")?;
                let _depth = ctx.enter_nested("rewrite native typed fields")?;
                $(self.$field = cadmpeg_ir::schema::rewrite::typed::RewriteIdentities::rewrite_identities(self.$field, ctx, map)?;)*
                Ok(self)
            }
        }
    };
    ($type:ty, [$($generic:ident),*]; ($($field:ident),* $(,)?)) => {
        impl<$($generic: cadmpeg_ir::schema::rewrite::typed::RewriteIdentities),*> cadmpeg_ir::schema::rewrite::typed::RewriteIdentities for $type {
            fn rewrite_native_value<RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(ctx: &cadmpeg_core::decode::DecodeContext<'_>, _value: &mut serde_json::Value, _map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>) -> Result<(), cadmpeg_core::CodecError> {
                ctx.charge_work(1, "walk native identity scalar")
            }
            fn visit_identity_references(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>) -> Result<(), cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("walk native typed references")?;
                ctx.charge_work(1, "walk native typed references")?;
                let Self($($field),*) = self;
                $(cadmpeg_ir::schema::rewrite::typed::RewriteIdentities::visit_identity_references($field, ctx, visitor)?;)*
                Ok(())
            }
            fn rewrite_identities<RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>) -> Result<Self, cadmpeg_core::CodecError> {
                ctx.charge_work(1, "rewrite native typed node")?;
                let _depth = ctx.enter_nested("rewrite native typed fields")?;
                let Self($($field),*) = self;
                Ok(Self($(cadmpeg_ir::schema::rewrite::typed::RewriteIdentities::rewrite_identities($field, ctx, map)?),*))
            }
        }
    };
}

macro_rules! rewrite_native_enum {
    ($type:ty, [$($generic:ident),*]; {$($variant:ident $(($($tuple:ident),*))? $({$($field:ident),*})?),* $(,)?} $(; native $native:expr)?) => {
        impl<$($generic: cadmpeg_ir::schema::rewrite::typed::RewriteIdentities),*> cadmpeg_ir::schema::rewrite::typed::RewriteIdentities for $type {
            fn rewrite_native_value<RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(ctx: &cadmpeg_core::decode::DecodeContext<'_>, _value: &mut serde_json::Value, _map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>) -> Result<(), cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("walk native typed variant")?;
                ctx.charge_work(1, "walk native typed variant")?;
                rewrite_native_wire!(ctx, _value, _map, Self, {}, [$($native)?])
            }
            fn visit_identity_references(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>) -> Result<(), cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("walk native typed references")?;
                ctx.charge_work(1, "walk native typed references")?;
                match self {
                    $(Self::$variant $(($($tuple),*))? $({$($field),*})? => {
                        $($(cadmpeg_ir::schema::rewrite::typed::RewriteIdentities::visit_identity_references($tuple, ctx, visitor)?;)*)?
                        $($(cadmpeg_ir::schema::rewrite::typed::RewriteIdentities::visit_identity_references($field, ctx, visitor)?;)*)?
                        Ok(())
                    },)*
                }
            }
            fn rewrite_identities<RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>) -> Result<Self, cadmpeg_core::CodecError> {
                ctx.charge_work(1, "rewrite native typed node")?;
                let _depth = ctx.enter_nested("rewrite native typed fields")?;
                match self {
                    $(Self::$variant $(($($tuple),*))? $({$($field),*})? => Ok(Self::$variant
                        $(($(cadmpeg_ir::schema::rewrite::typed::RewriteIdentities::rewrite_identities($tuple, ctx, map)?),*))?
                        $({$($field: cadmpeg_ir::schema::rewrite::typed::RewriteIdentities::rewrite_identities($field, ctx, map)?),*})?
                    ),)*
                }
            }
        }
    };
}

macro_rules! rewrite_native_wire {
    ($ctx:ident, $value:ident, $map:ident, $owner:ty, {$($field:ident),*}, [$handler:expr]) => { ($handler)($ctx, $value, $map) };
    ($ctx:ident, $value:ident, $map:ident, $owner:ty, {$($field:ident),*}, []) => {{
        $(cadmpeg_ir::schema::rewrite::typed::native_fields::rewrite_field($ctx, $value, stringify!($field), $map, |owner: &$owner| Some(&owner.$field))?;)*
        Ok(())
    }};
}
