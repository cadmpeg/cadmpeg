// SPDX-License-Identifier: Apache-2.0
//! The one door that mints the identities the NX decoder creates.

use cadmpeg_ir::ids::{
    Identity, IdentityComponent, IdentityKey, IdentityKeyTail, IdentityNamespace,
};
use cadmpeg_ir::{identity_component, identity_key};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use std::fmt::{self, Display, Write};

struct CountBytes(usize);

impl Write for CountBytes {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.0 = self.0.checked_add(text.len()).ok_or(fmt::Error)?;
        Ok(())
    }
}

/// The format component every NX identity carries.
fn nx() -> IdentityComponent {
    identity_component!("nx")
}

/// The `<format>:<scope>` half of every identity the NX decoder mints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IdScope(IdentityComponent);

impl IdScope {
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
        let mut count = CountBytes(0);
        write!(&mut count, "s{stream_index}")
            .map_err(|_| ctx.refuse_codec_limit("nx stream scope text", 0, u64::MAX))?;
        ctx.charge_retained(u64_from_index(count.0), "nx stream scope text")?;
        let mut text = String::new();
        text.try_reserve_exact(count.0).map_err(|_| {
            ctx.refuse_codec_limit("nx stream scope text", 0, u64_from_index(count.0))
        })?;
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

    /// The `<format>:<scope>` prefix this scope mints under.
    pub(super) fn prefix(&self) -> String {
        format!("{}:{}", nx().as_str(), self.0.as_str())
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

    /// Mint an NX identity after charging its retained text.
    pub(crate) fn id_charged<T: From<Identity>>(
        &self,
        ctx: &DecodeContext<'_>,
        kind: &IdentityComponent,
        key: impl Display,
    ) -> Result<T, CodecError> {
        let mut count = CountBytes(0);
        write!(&mut count, "nx:{}:{}#{key}", self.0.as_str(), kind.as_str())
            .map_err(|_| ctx.refuse_codec_limit("nx identity text", 0, u64::MAX))?;
        ctx.charge_retained(u64_from_index(count.0), "nx identity text")?;
        let mut text = String::new();
        text.try_reserve_exact(count.0)
            .map_err(|_| ctx.refuse_codec_limit("nx identity text", 0, u64_from_index(count.0)))?;
        write!(&mut text, "nx:{}:{}#{key}", self.0.as_str(), kind.as_str())
            .map_err(|_| ctx.refuse_codec_limit("nx identity text", 0, u64_from_index(count.0)))?;
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
