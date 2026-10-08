// SPDX-License-Identifier: Apache-2.0
//! The one door that mints the identities the NX decoder creates.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{
    Identity, IdentityComponent, IdentityKey, IdentityKeyTail, IdentityNamespace,
};
use cadmpeg_ir::{identity_component, identity_key};
use std::fmt::{self, Display, Write};

/// The format component every NX identity carries.
fn nx() -> IdentityComponent {
    identity_component!("nx")
}

/// The `<format>:<scope>` half of every identity the NX decoder mints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IdScope(IdentityComponent);

impl IdScope {
    /// Copy one decoded scope under the caller's retained-text limit.
    pub(crate) fn try_clone_for_decode(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        let text = ctx.copy_retained_text(self.0.as_str(), "nx completion scope copy")?;
        IdentityComponent::try_new(text)
            .map(Self)
            .map_err(CodecError::malformed)
    }

    /// The NX scope of the given name.
    pub(crate) fn native(scope: impl Into<IdentityComponent>) -> Self {
        Self(scope.into())
    }

    /// The scope of one Parasolid stream of the container.
    pub(crate) fn stream(stream_index: impl Into<IdentityComponent>) -> Self {
        Self::native(identity_component!("s").then(stream_index))
    }

    /// Build the scope of a Parasolid stream after charging its text.
    pub(crate) fn stream_charged(
        ctx: &DecodeContext<'_>,
        stream_index: usize,
    ) -> Result<Self, CodecError> {
        struct CountBytes(usize);

        impl Write for CountBytes {
            fn write_str(&mut self, text: &str) -> fmt::Result {
                self.0 = self.0.checked_add(text.len()).ok_or(fmt::Error)?;
                Ok(())
            }
        }

        // The stream index is a usize, so its decimal rendering is bounded by
        // usize::BITS digits plus the fixed `s` prefix on this target.
        let mut count = CountBytes(0);
        write!(&mut count, "s{stream_index}")
            .map_err(|_| ctx.refuse_codec_limit("nx stream scope text", 0, u64::MAX))?;
        let mut text = ctx.retained_string(count.0, "nx stream scope text")?;
        write!(&mut text, "s{stream_index}").map_err(|_| {
            ctx.refuse_codec_limit("nx stream scope text", 0, u64_from_index(count.0))
        })?;
        IdentityComponent::try_new(text)
            .map(Self::native)
            .map_err(CodecError::malformed)
    }

    /// The scope of whole-stream container evidence.
    pub(crate) fn container() -> Self {
        Self::native(identity_component!("container"))
    }

    /// The scope of entities the decoder synthesizes from non-topology input.
    pub(super) fn derived() -> Self {
        Self::native(identity_component!("derived"))
    }

    /// The scope an existing identity was minted under, when it is one.
    ///
    /// The prefix reaches this from stored record text, so it is admitted
    /// here rather than trusted.
    pub(crate) fn of(prefix: &str) -> Option<Self> {
        let scope = prefix.strip_prefix("nx:")?;
        let Ok(scope) = IdentityComponent::try_new(scope) else {
            return None;
        };
        Some(Self(scope))
    }

    /// Format this scope's prefix after admitting its work and retained bytes.
    pub(crate) fn prefix_charged(&self, ctx: &DecodeContext<'_>) -> Result<String, CodecError> {
        ctx.format_retained(format_args!("nx:{}", self.0.as_str()), "nx scope prefix")
    }

    /// Mint `<scope>:<kind>#<key>`.
    pub(crate) fn id<T: From<Identity>>(
        &self,
        kind: &IdentityComponent,
        key: impl Into<IdentityKey>,
    ) -> T {
        Identity::compose(
            &IdentityNamespace::from_components(&nx(), &self.0, kind),
            key,
        )
        .into()
    }

    /// Mint an NX identity after admitting its formatting work and retained bytes.
    pub(crate) fn id_charged<T: From<Identity>>(
        &self,
        ctx: &DecodeContext<'_>,
        kind: &IdentityComponent,
        key: impl Display,
    ) -> Result<T, CodecError> {
        let text = ctx.format_retained(
            format_args!("nx:{}:{}#{key}", self.0.as_str(), kind.as_str()),
            "nx identity text",
        )?;
        Identity::new(text)
            .map(Into::into)
            .map_err(CodecError::malformed)
    }

    /// Mint `<scope>:<kind>#<key>`, declining a key that leaves the grammar.
    pub(crate) fn try_id<T: From<Identity>>(
        &self,
        kind: &IdentityComponent,
        key: impl Into<String>,
    ) -> Option<T> {
        let Ok(key) = IdentityKey::try_new(key) else {
            return None;
        };
        Some(self.id(kind, key))
    }
}

/// Extend an existing entity id's key with a `:`-separated suffix.
///
/// The base reaches this as stored record text, so it is admitted here rather
/// than trusted; the suffix carries the key grammar already.
pub(crate) fn extended_id<T: From<Identity>>(base: &str, suffix: &IdentityKey) -> Option<T> {
    let Ok(base) = Identity::new(base) else {
        return None;
    };
    let tail = IdentityKeyTail::empty()
        .then(identity_key!(":"))
        .then(suffix);
    Some(base.with_key_tail(&tail).into())
}

/// The key that names an entity inside a native record, with the identity
/// separators flattened to `-`.
///
/// The text reaches this from stored record fields, so it is admitted here.
pub(crate) fn native_entity_key(id: &str) -> Option<IdentityKey> {
    let Ok(key) = IdentityKey::try_new(id.replace([':', '#'], "-")) else {
        return None;
    };
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::IdScope;
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::ids::{Identity, IdentityComponent, IdentityKey};

    fn long_scope() -> (IdScope, String) {
        // IdentityComponent has no maximum length; a decoded component can be
        // arbitrarily long while remaining nonempty and separator-free.
        let text = "scope".repeat(256);
        let component = IdentityComponent::try_new(text.clone()).expect("valid component");
        (IdScope::native(component), text)
    }

    #[test]
    fn charged_identity_formatters_admit_long_component_work_and_copy() {
        let (scope, scope_text) = long_scope();
        let key_text = "part".repeat(128);
        let key = IdentityKey::try_new(key_text.clone()).expect("valid key");
        let kind = cadmpeg_ir::identity_component!("face");

        for dimension in [ResourceDimension::WorkUnits, ResourceDimension::RetainedBytes] {
            let prefix_error = crate::test_support::resource_refusal_at(
                &[],
                dimension,
                "nx scope prefix",
                |ctx| scope.prefix_charged(ctx).map(|_| ()),
            );
            assert!(matches!(prefix_error, CodecError::ResourceLimit(limit)
                if limit.dimension == dimension && limit.operation == "nx scope prefix"));

            let id_error = crate::test_support::resource_refusal_at(
                &[],
                dimension,
                "nx identity text",
                |ctx| {
                    scope
                        .id_charged::<Identity>(ctx, &kind, key.clone())
                        .map(|_| ())
                },
            );
            assert!(matches!(id_error, CodecError::ResourceLimit(limit)
                if limit.dimension == dimension && limit.operation == "nx identity text"));
        }

        assert_eq!(scope.0.as_str(), scope_text);
    }

    #[test]
    fn charged_identity_formatters_preserve_exact_text_and_admit_each_copy() {
        let scope = IdScope::native(IdentityComponent::try_new("s2").expect("valid scope"));
        let kind = cadmpeg_ir::identity_component!("face");
        let key = IdentityKey::try_new("partition-0123456789".to_owned()).expect("valid key");
        let expected = "nx:s2:face#partition-0123456789";
        let expected_work = 2 * expected.len();
        let expected_prefix = "nx:s2";
        let expected_prefix_work = 2 * expected_prefix.len();

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_work_units =
                    u64::try_from(expected_prefix_work).expect("prefix work fits u64");
            },
            |ctx| {
                let prefix = scope
                    .prefix_charged(ctx)
                    .expect("exact prefix formatting work is admitted");
                assert_eq!(prefix, expected_prefix);
                let error = ctx
                    .charge_work(1, "probe NX scope prefix formatting work")
                    .expect_err("formatted prefix work is fully charged");
                assert!(matches!(error, CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::WorkUnits
                        && limit.used
                            == u64::try_from(expected_prefix_work).expect("prefix work fits u64")
                        && limit.additional == 1));
            },
        );

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_retained_bytes =
                    u64::try_from(expected_prefix.len()).expect("prefix length fits u64");
            },
            |ctx| {
                let prefix = scope
                    .prefix_charged(ctx)
                    .expect("exact prefix copy fits");
                assert_eq!(prefix, expected_prefix);
                let error = ctx
                    .charge_retained(1, "probe NX scope prefix retained copy")
                    .expect_err("the formatted prefix remains retained");
                assert!(matches!(error, CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::RetainedBytes
                        && limit.used
                            == u64::try_from(expected_prefix.len())
                                .expect("prefix length fits u64")
                        && limit.additional == 1));
            },
        );

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_work_units =
                    u64::try_from(expected_work).expect("identity work fits u64");
            },
            |ctx| {
                let id = scope
                    .id_charged::<Identity>(ctx, &kind, key.clone())
                    .expect("exact formatting work is admitted");
                assert_eq!(id.as_str(), expected);
                let error = ctx
                    .charge_work(1, "probe NX identity formatting work")
                    .expect_err("formatted text work is fully charged");
                assert!(matches!(error, CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::WorkUnits
                        && limit.used
                            == u64::try_from(expected_work).expect("identity work fits u64")
                        && limit.additional == 1));
            },
        );

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_retained_bytes =
                    u64::try_from(expected.len()).expect("identity length fits u64");
            },
            |ctx| {
                let id = scope
                    .id_charged::<Identity>(ctx, &kind, key.clone())
                    .expect("exact formatted copy fits");
                assert_eq!(id.as_str(), expected);
                let error = ctx
                    .charge_retained(1, "probe NX identity retained copy")
                    .expect_err("the formatted identity remains retained");
                assert!(matches!(error, CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::RetainedBytes
                        && limit.used
                            == u64::try_from(expected.len()).expect("identity length fits u64")
                        && limit.additional == 1));
            },
        );
    }

    #[test]
    fn charged_stream_scope_keeps_fixed_usize_formatting() {
        let expected = format!("s{}", usize::MAX);
        let scope = crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_work_units = 0;
                policy.limits.max_retained_bytes =
                    u64::try_from(expected.len()).expect("stream scope length fits u64");
            },
            |ctx| IdScope::stream_charged(ctx, usize::MAX),
        )
        .expect("fixed-width usize rendering is retained");
        let prefix = crate::test_support::with_decode_context(|ctx| {
            scope.prefix_charged(ctx)
        })
        .expect("stream scope can be copied");
        assert_eq!(prefix, format!("nx:{expected}"));
    }
}
