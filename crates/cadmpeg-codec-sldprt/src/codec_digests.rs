// SPDX-License-Identifier: Apache-2.0
//! The codec digests (§3.1, Rule B) the SLDPRT decoder records for its patch
//! path, and the writer's lookup of them. `docs/formats/sldprt.md` §12 lists
//! the keys.
//!
//! The decode stages record each digest among the source attributes of the
//! document they build; the decode moves them into the sidecar before it
//! returns, so the document holds none. A write of a part given no sidecar
//! takes them from the document it writes.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Decoded;
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::source_fidelity::CodecDigests;
use cadmpeg_ir::CadIr;

/// Every key under which the decoder records a codec digest.
pub(crate) const KEYS: [&str; 16] = [
    "brep_local_sha256",
    "sldprt_configuration_feature_states_local_sha256",
    "sldprt_configuration_parameter_values_local_sha256",
    "sldprt_native_configuration_sha256",
    "sldprt_native_history_sha256",
    "sldprt_native_parameter_sha256",
    "sldprt_native_sketch_sha256",
    "sldprt_neutral_configuration_local_sha256",
    "sldprt_neutral_feature_local_sha256",
    "sldprt_neutral_parameter_local_sha256",
    "sldprt_neutral_sketch_constraint_local_sha256",
    "sldprt_neutral_sketch_local_sha256",
    crate::writer::PMI_LOCAL_DIGEST_ATTRIBUTE,
    crate::writer::SWOBJECTS_LOCAL_DIGEST_ATTRIBUTE,
    crate::writer::SWOBJECTS_MATERIAL_LOCAL_DIGEST_ATTRIBUTE,
    crate::writer::SWOBJECTS_METADATA_IDENTITY_LOCAL_DIGEST_ATTRIBUTE,
];

/// Moves each codec digest the decode stages recorded among the document's
/// source attributes into the sidecar's codec digests.
pub(crate) fn move_to_sidecar(decoded: &mut Decoded) -> Result<(), CodecError> {
    let digests = take(&mut decoded.ir)?;
    decoded.source_fidelity.codec_digests.extend(digests);
    Ok(())
}

/// Takes each codec digest recorded among the source attributes of `ir` out
/// of them.
fn take(ir: &mut CadIr) -> Result<CodecDigests, CodecError> {
    let mut digests = CodecDigests::new();
    let Some(source) = ir.source.as_mut() else {
        return Ok(digests);
    };
    for key in KEYS {
        if let Some((key, value)) = source.attributes.remove_entry(key) {
            let digest = sha256(&key, value)?;
            digests.insert(key, digest);
        }
    }
    Ok(digests)
}

/// Whether `ir` holds a `SolidWorks` feature history or feature-input lane,
/// as a document decoded from a part does.
pub(crate) fn holds_history(ir: &CadIr) -> bool {
    ir.native.namespace("sldprt").is_some_and(|namespace| {
        ["feature_histories", "feature_input_lanes"]
            .iter()
            .any(|arena| {
                namespace
                    .arenas()
                    .get(*arena)
                    .is_some_and(|records| !records.is_empty())
            })
    })
}

/// The codec digests a decode of the part `ir` states records, taken from `ir`
/// itself. A write given no sidecar compares against them, so it reads the
/// document as unedited (§3.4, Paths: the patch that holds no digest).
pub(crate) fn of_document(ir: &CadIr) -> Result<CodecDigests, CodecError> {
    use crate::history::hash::{
        configuration_feature_state_hash, configuration_hash, configuration_parameter_value_hash,
        feature_hash, history_hash, native_configuration_hash, native_parameter_hash,
        parameter_hash,
    };
    use crate::resolved_features::hashes::{constraint_hash, lane_hash, sketch_hash};

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())?;
    let native = ir
        .native
        .namespace("sldprt")
        .map(|namespace| {
            crate::native::SldprtNative::load(namespace, ir.tolerances.linear)
                .map_err(CodecError::from)
        })
        .transpose()?
        .unwrap_or_default();
    let histories = &native.feature_histories;
    let configurations = crate::writer::configurations_without_synthesized_snapshot(ir);
    let lanes = [
        (
            "sldprt_neutral_feature_local_sha256",
            feature_hash(&ctx, &ir.model)?,
        ),
        (
            "sldprt_native_history_sha256",
            history_hash(&ctx, histories)?,
        ),
        (
            "sldprt_native_configuration_sha256",
            native_configuration_hash(&ctx, histories)?,
        ),
        (
            "sldprt_neutral_parameter_local_sha256",
            parameter_hash(&ctx, &ir.model.parameters)?,
        ),
        (
            "sldprt_native_parameter_sha256",
            native_parameter_hash(&ctx, histories)?,
        ),
        (
            "sldprt_neutral_configuration_local_sha256",
            configuration_hash(&ctx, &configurations)?,
        ),
        (
            "sldprt_configuration_parameter_values_local_sha256",
            configuration_parameter_value_hash(&ctx, &configurations)?,
        ),
        (
            "sldprt_configuration_feature_states_local_sha256",
            configuration_feature_state_hash(&ctx, &configurations)?,
        ),
        ("sldprt_neutral_sketch_local_sha256", sketch_hash(&ctx, ir)?),
        (
            "sldprt_native_sketch_sha256",
            lane_hash(&ctx, &native.feature_input_lanes)?,
        ),
        (
            "sldprt_neutral_sketch_constraint_local_sha256",
            constraint_hash(&ctx, ir)?,
        ),
    ];
    let mut document = ir.clone();
    crate::decode::stamp_local_digests(&ctx, &mut document)?;
    let mut digests = take(&mut document)?;
    for (key, value) in lanes {
        let key = NonBlankString::new(key)
            .ok_or_else(|| CodecError::malformed(format_args!("blank codec digest key {key:?}")))?;
        let digest = sha256(&key, value)?;
        digests.insert(key, digest);
    }
    Ok(digests)
}

fn sha256(key: &NonBlankString, value: String) -> Result<Sha256Digest, CodecError> {
    Sha256Digest::try_from(value).map_err(|_| {
        CodecError::malformed(format_args!(
            "SLDPRT codec digest {key} is no SHA-256 digest"
        ))
    })
}

/// The digest the decoder recorded under `key`, as lower-case hexadecimal.
pub(crate) fn baseline<'a>(digests: &'a CodecDigests, key: &str) -> Option<&'a str> {
    digests.get(key).map(Sha256Digest::as_str)
}
