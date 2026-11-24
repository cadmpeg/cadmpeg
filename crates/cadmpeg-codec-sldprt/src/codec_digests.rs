// SPDX-License-Identifier: Apache-2.0
//! The codec digests (§3.1, Rule B) the SLDPRT decoder records for its patch
//! path, and the writer's lookup of them. `docs/formats/sldprt.md` §12 lists
//! the keys.
//!
//! The decode stages record each digest among the source attributes of the
//! document they build; the decode moves them into the sidecar before it
//! returns, so the document holds none.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Decoded;
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::source_fidelity::CodecDigests;

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
    let Some(source) = decoded.ir.source.as_mut() else {
        return Ok(());
    };
    for key in KEYS {
        if let Some((key, value)) = source.attributes.remove_entry(key) {
            let digest = Sha256Digest::try_from(value).map_err(|_| {
                CodecError::malformed(format_args!(
                    "SLDPRT codec digest {key} is no SHA-256 digest"
                ))
            })?;
            decoded.source_fidelity.codec_digests.insert(key, digest);
        }
    }
    Ok(())
}

/// The digest the decoder recorded under `key`, as lower-case hexadecimal.
pub(crate) fn baseline<'a>(digests: &'a CodecDigests, key: &str) -> Option<&'a str> {
    digests.get(key).map(Sha256Digest::as_str)
}
