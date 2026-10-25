// SPDX-License-Identifier: Apache-2.0
//! Direct field implementations for the typed identity rewrite contract.

macro_rules! rewrite_scalar {
    ($type:ty) => {
        impl crate::schema::rewrite::typed::RewriteIdentities for $type {
            const HAS_IDENTITY_FIELDS: bool = false;
            fn rewrite_native_value<
                RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>,
            >(
                _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
                _value: &mut serde_json::Value,
                _map: &mut crate::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>,
            ) -> Result<(), cadmpeg_core::CodecError> {
                Ok(())
            }
            fn visit_identity_references(
                &self,
                _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
                _visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>,
            ) -> Result<(), cadmpeg_core::CodecError> {
                Ok(())
            }
            fn rewrite_identities<RewriteMapFn>(
                self,
                _rewrite_context: &cadmpeg_core::decode::DecodeContext<'_>,
                _identity_map: &mut crate::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>,
            ) -> Result<Self, cadmpeg_core::CodecError>
            where
                RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>,
            {
                Ok(self)
            }
        }
    };
}

// Full-fidelity references use string wire fields, but their owner declares
// identity semantics. Ordinary display/source text remains ordinary text.
macro_rules! rewrite_record_field {
    (native_ref, $value:expr, $ctx:expr, $map:expr) => {
        crate::schema::rewrite::typed::native_fields::FullFidelityReference::rewrite($value, $map)
    };
    (geometry_ref, $value:expr, $ctx:expr, $map:expr) => {
        crate::schema::rewrite::typed::native_fields::FullFidelityReference::rewrite($value, $map)
    };
    (endpoint_refs, $value:expr, $ctx:expr, $map:expr) => {
        crate::schema::rewrite::typed::native_fields::FullFidelityReference::rewrite($value, $map)
    };
    ($field:ident, $value:expr, $ctx:expr, $map:expr) => {
        crate::schema::rewrite::typed::RewriteIdentities::rewrite_identities($value, $ctx, $map)
    };
}

macro_rules! rewrite_native_record_field {
    (native_ref, $ctx:expr, $value:expr, $map:expr, $owner:ty) => {
        crate::schema::rewrite::typed::native_fields::rewrite_reference($value, "native_ref", $map)
    };
    (geometry_ref, $ctx:expr, $value:expr, $map:expr, $owner:ty) => {
        crate::schema::rewrite::typed::native_fields::rewrite_reference(
            $value,
            "geometry_ref",
            $map,
        )
    };
    (endpoint_refs, $ctx:expr, $value:expr, $map:expr, $owner:ty) => {
        crate::schema::rewrite::typed::native_fields::rewrite_reference(
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

macro_rules! visit_dependency_field {
    (native_ref, $value:expr, $ctx:expr, $visitor:expr) => {
        crate::schema::rewrite::typed::prune::DeclaredReferenceField::visit($value, $ctx, $visitor)
    };
    (geometry_ref, $value:expr, $ctx:expr, $visitor:expr) => {
        crate::schema::rewrite::typed::prune::DeclaredReferenceField::visit($value, $ctx, $visitor)
    };
    (endpoint_refs, $value:expr, $ctx:expr, $visitor:expr) => {
        crate::schema::rewrite::typed::prune::DeclaredReferenceField::visit($value, $ctx, $visitor)
    };
    ($field:ident, $value:expr, $ctx:expr, $visitor:expr) => {
        crate::schema::rewrite::typed::RewriteIdentities::visit_dependency_references(
            $value, $ctx, $visitor,
        )
    };
}
macro_rules! apply_dependency_field {
    (native_ref, $value:expr, $ctx:expr, $deleted:expr) => {
        crate::schema::rewrite::typed::prune::DeclaredReferenceField::apply($value, $ctx, $deleted)
    };
    (geometry_ref, $value:expr, $ctx:expr, $deleted:expr) => {
        crate::schema::rewrite::typed::prune::DeclaredReferenceField::apply($value, $ctx, $deleted)
    };
    (endpoint_refs, $value:expr, $ctx:expr, $deleted:expr) => {
        crate::schema::rewrite::typed::prune::DeclaredReferenceField::apply($value, $ctx, $deleted)
    };
    ($field:ident, $value:expr, $ctx:expr, $deleted:expr) => {
        crate::schema::rewrite::typed::RewriteIdentities::prune_reference_lists(
            $value, $ctx, $deleted,
        )
    };
}

macro_rules! allow_tuple_list_pruning {
    () => {
        false
    };
    (allow_list_pruning) => {
        true
    };
}
macro_rules! rewrite_record {
    ($type:ty, [$($generic:ident $(: $bound:path)?),*]; {$($field:ident),* $(,)?}) => {
        impl<$($generic: crate::schema::rewrite::typed::RewriteIdentities $(+ $bound)?),*> crate::schema::rewrite::typed::RewriteIdentities for $type {
            fn visit_dependency_references(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, visitor: &mut dyn FnMut(crate::schema::rewrite::typed::prune::DependencyReference<'_>) -> Result<(), cadmpeg_core::CodecError>) -> Result<(), cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("walk record dependency fields")?;
                $(visit_dependency_field!($field, &self.$field, ctx, &mut |mut reference: crate::schema::rewrite::typed::prune::DependencyReference<'_>| {
                    if reference.field.is_none() { reference.field = Some(stringify!($field)); }
                    visitor(reference)
                })?;)*
                Ok(())
            }
            fn prune_reference_lists(&mut self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, deleted: &std::collections::BTreeSet<String>) -> Result<crate::schema::rewrite::typed::prune::ReferencePruneEffect, cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("prune record dependency lists")?;
                let effect = crate::schema::rewrite::typed::prune::analyze(self, ctx, deleted)?;
                if effect.can_apply() { $(apply_dependency_field!($field, &mut self.$field, ctx, deleted)?;)* }
                Ok(effect)
            }
            fn validate_dimensions(&self, ir: &crate::CadIr, ctx: &cadmpeg_core::decode::DecodeContext<'_>, errors: &mut Vec<crate::features::DimensionResolutionError>) -> Result<(), cadmpeg_core::decode::ResourceLimit> {
                let _depth = ctx.enter_nested_limit("check dimensional record")?;
                $(crate::schema::rewrite::typed::RewriteIdentities::validate_dimensions(&self.$field, ir, ctx, errors)?;)*
                Ok(())
            }
            fn resolve_dimensions(self, ir: &crate::CadIr, ctx: &cadmpeg_core::decode::DecodeContext<'_>) -> Result<Self, crate::features::DimensionResolutionError> {
                let _depth = ctx.enter_nested_limit("resolve dimensional record")?;
                let Self { $($field),* } = self;
                Ok(Self { $($field: crate::schema::rewrite::typed::RewriteIdentities::resolve_dimensions($field, ir, ctx)?),* })
            }
            fn rewrite_native_value<RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(ctx: &cadmpeg_core::decode::DecodeContext<'_>, value: &mut serde_json::Value, map: &mut crate::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>) -> Result<(), cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("walk native typed fields")?;
                $(rewrite_native_record_field!($field, ctx, value, map, Self)?;)*
                Ok(())
            }
            fn visit_identity_references(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>) -> Result<(), cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("walk typed reference fields")?;
                $(crate::schema::rewrite::typed::RewriteIdentities::visit_identity_references(&self.$field, ctx, visitor)?;)*
                Ok(())
            }
            fn visit_local_members(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>) -> Result<(), cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("walk typed local member fields")?;
                $(crate::schema::rewrite::typed::RewriteIdentities::visit_local_members(&self.$field, ctx, visitor)?;)*
                Ok(())
            }
            fn rewrite_identities<RewriteMapFn>(self, rewrite_context: &cadmpeg_core::decode::DecodeContext<'_>, identity_map: &mut crate::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>) -> Result<Self, cadmpeg_core::CodecError>
            where RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError> {
                let _depth = rewrite_context.enter_nested("typed rewrite record")?;
                let Self { $($field),* } = self;
                Ok(Self { $($field: rewrite_record_field!($field, $field, rewrite_context, identity_map)?),* })
            }
        }
    };
    ($type:ty, [$($generic:ident $(: $bound:path)?),*]; ($($field:ident),* $(,)?) $(; $prune_mode:ident)?) => {
        impl<$($generic: crate::schema::rewrite::typed::RewriteIdentities $(+ $bound)?),*> crate::schema::rewrite::typed::RewriteIdentities for $type {
            fn visit_dependency_references(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, visitor: &mut dyn FnMut(crate::schema::rewrite::typed::prune::DependencyReference<'_>) -> Result<(), cadmpeg_core::CodecError>) -> Result<(), cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("walk checked tuple dependencies")?;
                let Self($($field),*) = self;
                let allow_pruning = allow_tuple_list_pruning!($($prune_mode)?);
                $(crate::schema::rewrite::typed::RewriteIdentities::visit_dependency_references($field, ctx, &mut |mut reference| {
                    if let crate::schema::rewrite::typed::prune::DependencyPosition::List { length, index } = reference.position {
                        if !allow_pruning { reference.position = crate::schema::rewrite::typed::prune::DependencyPosition::UnrepresentablePrunedList { owner: std::any::type_name::<Self>(), length, index }; }
                    }
                    visitor(reference)
                })?;)*
                Ok(())
            }
            fn prune_reference_lists(&mut self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, deleted: &std::collections::BTreeSet<String>) -> Result<crate::schema::rewrite::typed::prune::ReferencePruneEffect, cadmpeg_core::CodecError> {
                let effect = crate::schema::rewrite::typed::prune::analyze(self, ctx, deleted)?;
                if effect.can_apply() { let Self($($field),*) = self; $(crate::schema::rewrite::typed::RewriteIdentities::prune_reference_lists($field, ctx, deleted)?;)* }
                Ok(effect)
            }
            fn validate_dimensions(&self, ir: &crate::CadIr, ctx: &cadmpeg_core::decode::DecodeContext<'_>, errors: &mut Vec<crate::features::DimensionResolutionError>) -> Result<(), cadmpeg_core::decode::ResourceLimit> {
                let _depth = ctx.enter_nested_limit("check dimensional tuple owner")?;
                let Self($($field),*) = self;
                $(crate::schema::rewrite::typed::RewriteIdentities::validate_dimensions($field, ir, ctx, errors)?;)*
                Ok(())
            }
            fn resolve_dimensions(self, ir: &crate::CadIr, ctx: &cadmpeg_core::decode::DecodeContext<'_>) -> Result<Self, crate::features::DimensionResolutionError> {
                let _depth = ctx.enter_nested_limit("resolve dimensional tuple owner")?;
                let Self($($field),*) = self;
                Ok(Self($(crate::schema::rewrite::typed::RewriteIdentities::resolve_dimensions($field, ir, ctx)?),*))
            }
            fn visit_identity_references(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>) -> Result<(), cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("walk typed reference fields")?;
                let Self($($field),*) = self;
                $(crate::schema::rewrite::typed::RewriteIdentities::visit_identity_references($field, ctx, visitor)?;)*
                Ok(())
            }
            fn visit_local_members(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>) -> Result<(), cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("walk typed local member fields")?;
                let Self($($field),*) = self;
                $(crate::schema::rewrite::typed::RewriteIdentities::visit_local_members($field, ctx, visitor)?;)*
                Ok(())
            }
            fn rewrite_identities<RewriteMapFn>(self, rewrite_context: &cadmpeg_core::decode::DecodeContext<'_>, identity_map: &mut crate::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>) -> Result<Self, cadmpeg_core::CodecError>
            where RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError> {
                let _depth = rewrite_context.enter_nested("typed rewrite tuple")?;
                let Self($($field),*) = self;
                Ok(Self($(crate::schema::rewrite::typed::RewriteIdentities::rewrite_identities($field, rewrite_context, identity_map)?),*))
            }
        }
    };
}

macro_rules! reference_enum_leaf {
    () => {
        false
    };
    ($leaf:literal) => {
        $leaf
    };
}
macro_rules! rewrite_enum {
    ($type:ty, []; {$($variant:ident),* $(,)?}) => {
        rewrite_scalar!($type);
    };
    ($type:ty, [$($generic:ident $(: $bound:path)?),*]; {
        $($variant:ident $(($($tuple:ident),*))? $({$($field:ident),*})?),* $(,)?
    } $(; reference_leaf $reference_leaf:literal; string_references { $($ref_variant:ident($ref_field:ident)),* $(,)? })?) => {
        impl<$($generic: crate::schema::rewrite::typed::RewriteIdentities $(+ $bound)?),*> crate::schema::rewrite::typed::RewriteIdentities for $type {
            const IS_REFERENCE_LEAF: bool = reference_enum_leaf!($($reference_leaf)?);
            fn visit_dependency_references(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, visitor: &mut dyn FnMut(crate::schema::rewrite::typed::prune::DependencyReference<'_>) -> Result<(), cadmpeg_core::CodecError>) -> Result<(), cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("walk variant dependency fields")?;
                $(match self { $(Self::$ref_variant($ref_field) => crate::schema::rewrite::typed::prune::DeclaredReferenceField::visit($ref_field, ctx, visitor)?,)* _ => {} })?

                match self { $(Self::$variant $(($($tuple),*))? $({$($field),*})? => {
                    $($(crate::schema::rewrite::typed::RewriteIdentities::visit_dependency_references($tuple, ctx, visitor)?;)*)?
                    $($(visit_dependency_field!($field, $field, ctx, &mut |mut reference: crate::schema::rewrite::typed::prune::DependencyReference<'_>| {
                        if reference.field.is_none() { reference.field = Some(stringify!($field)); }
                        visitor(reference)
                    })?;)*)?
                    Ok(())
                },)* }
            }
            fn prune_reference_lists(&mut self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, deleted: &std::collections::BTreeSet<String>) -> Result<crate::schema::rewrite::typed::prune::ReferencePruneEffect, cadmpeg_core::CodecError> {
                let effect = crate::schema::rewrite::typed::prune::analyze(self, ctx, deleted)?;
                if !effect.can_apply() { return Ok(effect); }
                match self { $(Self::$variant $(($($tuple),*))? $({$($field),*})? => {
                    $($(crate::schema::rewrite::typed::RewriteIdentities::prune_reference_lists($tuple, ctx, deleted)?;)*)?
                    $($(apply_dependency_field!($field, $field, ctx, deleted)?;)*)?
                },)* }
                Ok(effect)
            }
            fn validate_dimensions(&self, ir: &crate::CadIr, ctx: &cadmpeg_core::decode::DecodeContext<'_>, errors: &mut Vec<crate::features::DimensionResolutionError>) -> Result<(), cadmpeg_core::decode::ResourceLimit> {
                let _depth = ctx.enter_nested_limit("check dimensional variant")?;
                match self {
                    $(Self::$variant $(($($tuple),*))? $({$($field),*})? => {
                        $($(crate::schema::rewrite::typed::RewriteIdentities::validate_dimensions($tuple, ir, ctx, errors)?;)*)?
                        $($(crate::schema::rewrite::typed::RewriteIdentities::validate_dimensions($field, ir, ctx, errors)?;)*)?
                        Ok(())
                    },)*
                }
            }
            fn resolve_dimensions(self, ir: &crate::CadIr, ctx: &cadmpeg_core::decode::DecodeContext<'_>) -> Result<Self, crate::features::DimensionResolutionError> {
                let _depth = ctx.enter_nested_limit("resolve dimensional variant")?;
                match self {
                    $(Self::$variant $(($($tuple),*))? $({$($field),*})? => Ok(Self::$variant
                        $(($(crate::schema::rewrite::typed::RewriteIdentities::resolve_dimensions($tuple, ir, ctx)?),*))?
                        $({$($field: crate::schema::rewrite::typed::RewriteIdentities::resolve_dimensions($field, ir, ctx)?),*})?
                    ),)*
                }
            }
            fn visit_identity_references(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>) -> Result<(), cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("walk typed reference fields")?;
                match self {
                    $(Self::$variant $(($($tuple),*))? $({$($field),*})? => {
                        $($(crate::schema::rewrite::typed::RewriteIdentities::visit_identity_references($tuple, ctx, visitor)?;)*)?
                        $($(crate::schema::rewrite::typed::RewriteIdentities::visit_identity_references($field, ctx, visitor)?;)*)?
                        Ok(())
                    },)*
                }
            }
            fn visit_local_members(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>) -> Result<(), cadmpeg_core::CodecError> {
                let _depth = ctx.enter_nested("walk typed local member fields")?;
                match self {
                    $(Self::$variant $(($($tuple),*))? $({$($field),*})? => {
                        $($(crate::schema::rewrite::typed::RewriteIdentities::visit_local_members($tuple, ctx, visitor)?;)*)?
                        $($(crate::schema::rewrite::typed::RewriteIdentities::visit_local_members($field, ctx, visitor)?;)*)?
                        Ok(())
                    },)*
                }
            }
            fn rewrite_identities<RewriteMapFn>(self, rewrite_context: &cadmpeg_core::decode::DecodeContext<'_>, identity_map: &mut crate::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>) -> Result<Self, cadmpeg_core::CodecError>
            where RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError> {
                let _depth = rewrite_context.enter_nested("typed rewrite variant")?;
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
