// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_ir::ids::{IdentityComponent, IdentityKey, IdentityKeyTail, IdentityNamespace, StaticIdentityComponent};

const STATIC_COMPONENT: StaticIdentityComponent = match StaticIdentityComponent::new("cad-body") {
    Some(component) => component,
    None => panic!("valid fixture component"),
};

pub fn static_component_from_const(_ctx: &DecodeContext<'_>) -> IdentityComponent {
    IdentityComponent::from_static(STATIC_COMPONENT)
}

pub fn exact_runtime_charge(
    ctx: &DecodeContext<'_>,
    source: String,
) -> Result<(), cadmpeg_core::CodecError> {
    let work = u64_from_index(source.len()).checked_mul(2).ok_or_else(|| ctx.refuse_codec_limit("component grammar extent", u64::MAX, 1))?;
    ctx.charge_work(work, "component grammar")?;
    let _component = IdentityComponent::try_new(source);
    Ok(())
}

pub fn wrong_runtime_charge(
    ctx: &DecodeContext<'_>,
    source: String,
    other: &str,
) -> Result<(), cadmpeg_core::CodecError> {
    let work = u64_from_index(other.len()).checked_mul(2).ok_or_else(|| ctx.refuse_codec_limit("unrelated grammar extent", u64::MAX, 1))?;
    ctx.charge_work(work, "unrelated grammar")?;
    let _component = IdentityComponent::try_new(source); // finding: uncharged_decode_work
    Ok(())
}

pub fn insufficient_runtime_charge(
    ctx: &DecodeContext<'_>,
    source: String,
) -> Result<(), cadmpeg_core::CodecError> {
    let work = u64_from_index(source.len());
    ctx.charge_work(work, "one component grammar scan")?;
    let _component = IdentityComponent::try_new(source); // finding: uncharged_decode_work
    Ok(())
}

pub fn exact_key_grammar_charges(
    ctx: &DecodeContext<'_>,
    key: String,
    tail: String,
) -> Result<(), cadmpeg_core::CodecError> {
    let key_work = u64_from_index(key.len()).checked_mul(2).ok_or_else(|| ctx.refuse_codec_limit("key grammar extent", u64::MAX, 1))?;
    ctx.charge_work(key_work, "key grammar")?;
    let _key = IdentityKey::try_new(key);
    let tail_work = u64_from_index(tail.len()).checked_mul(2).ok_or_else(|| ctx.refuse_codec_limit("tail grammar extent", u64::MAX, 1))?;
    ctx.charge_work(tail_work, "tail grammar")?;
    let _tail = IdentityKeyTail::try_new(tail);
    Ok(())
}

pub fn uncharged_key_grammar(_ctx: &DecodeContext<'_>, key: String) {
    let _key = IdentityKey::try_new(key); // finding: uncharged_decode_work
}

pub fn exact_namespace_grammar_charges(
    ctx: &DecodeContext<'_>,
    format: String,
    scope: String,
    kind: String,
) -> Result<(), cadmpeg_core::CodecError> {
    let work = u64_from_index(format.len()).checked_mul(2).ok_or_else(|| ctx.refuse_codec_limit("format grammar extent", u64::MAX, 1))?;
    ctx.charge_work(work, "format grammar")?;
    let work = u64_from_index(scope.len()).checked_mul(2).ok_or_else(|| ctx.refuse_codec_limit("scope grammar extent", u64::MAX, 1))?;
    ctx.charge_work(work, "scope grammar")?;
    let work = u64_from_index(kind.len()).checked_mul(2).ok_or_else(|| ctx.refuse_codec_limit("kind grammar extent", u64::MAX, 1))?;
    ctx.charge_work(work, "kind grammar")?;
    let _namespace = IdentityNamespace::new(format, scope, kind);
    Ok(())
}

pub fn missing_namespace_source_charge(
    ctx: &DecodeContext<'_>,
    format: String,
    scope: String,
    kind: String,
) -> Result<(), cadmpeg_core::CodecError> {
    let work = u64_from_index(format.len()).checked_mul(2).ok_or_else(|| ctx.refuse_codec_limit("format grammar extent", u64::MAX, 1))?;
    ctx.charge_work(work, "format grammar")?;
    let work = u64_from_index(scope.len()).checked_mul(2).ok_or_else(|| ctx.refuse_codec_limit("scope grammar extent", u64::MAX, 1))?;
    ctx.charge_work(work, "scope grammar")?;
    let _namespace = IdentityNamespace::new(format, scope, kind); // finding: uncharged_decode_work
    Ok(())
}
