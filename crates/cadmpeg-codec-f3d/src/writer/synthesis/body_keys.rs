// SPDX-License-Identifier: Apache-2.0
//! Assign unique active body and browser identities to the synthesized stream.

use std::collections::BTreeSet;

use cadmpeg_asm::brep::records::identity::NativeRecordNamespace;
use cadmpeg_asm::brep::records::BodyNativeKey;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;

use crate::native::F3dNative;
use crate::records::bodies::{BodyVisibility, BodyVisibilityContent};

pub(super) fn reconcile(target: &mut CadIr, native: &mut F3dNative) -> Result<(), CodecError> {
    // Key row for a neutral body, resolved through the body's `native_ref`.
    fn key_for<'a>(
        keys: &'a [BodyNativeKey],
        body: &cadmpeg_ir::topology::Body,
    ) -> Option<&'a BodyNativeKey> {
        keys.iter()
            .find(|key| key.owns_native_ref(body.native_ref.as_deref()))
    }
    // Reserve explicit choices before assigning defaults. An earlier body
    // without a key must not consume a later body's nonconflicting identity.
    let reserved_keys = target
        .model
        .bodies
        .iter()
        .filter_map(|body| key_for(&native.body_native_keys, body).and_then(|row| row.asm_body_key))
        .collect::<BTreeSet<_>>();
    let reserved_suffixes = target
        .model
        .bodies
        .iter()
        .filter(|body| body.visible.is_some())
        .filter_map(|body| {
            key_for(&native.body_native_keys, body).and_then(|key| {
                native
                    .body_visibilities
                    .iter()
                    .find(|visibility| visibility.body.as_str() == key.id().as_str())
            })
        })
        .map(|row| row.entity_suffix)
        .collect::<BTreeSet<_>>();
    let mut used_keys = BTreeSet::new();
    let mut used_suffixes = BTreeSet::new();
    let namespace = NativeRecordNamespace::new(cadmpeg_asm::asm_format!("f3d"));
    let mut record_indices = native
        .body_native_keys
        .iter()
        .filter(|record| record.source_namespace == namespace)
        .map(|record| u64::from(record.record_index))
        .collect::<BTreeSet<_>>();
    let mut body_keys = Vec::with_capacity(target.model.bodies.len());
    let mut body_visibilities = Vec::new();
    for (ordinal, body) in target.model.bodies.iter_mut().enumerate() {
        let default_key = u64::try_from(ordinal)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| CodecError::InvalidInput("F3D body ordinal exceeds u64".into()))?;
        let original = key_for(&native.body_native_keys, body).cloned();
        let preferred = original.as_ref().and_then(|record| record.asm_body_key);
        let key = assign(
            preferred,
            default_key,
            &reserved_keys,
            &mut used_keys,
            u64::MAX / 2,
            "ASM body key",
        )?;
        let record = match original {
            Some(original) => {
                let mut record = original.clone();
                record.asm_body_key = Some(key);
                record
            }
            None => {
                let record_index = assign(
                    None,
                    default_key,
                    &BTreeSet::new(),
                    &mut record_indices,
                    u64::from(u32::MAX),
                    "body record index",
                )?;
                BodyNativeKey {
                    source_namespace: namespace.clone(),
                    record_index: u32::try_from(record_index).map_err(|_| {
                        CodecError::InvalidInput("F3D body record index exceeds u32".into())
                    })?,
                    body_ordinal: u32::try_from(ordinal).map_err(|_| {
                        CodecError::InvalidInput("F3D body ordinal exceeds u32".into())
                    })?,
                    source_brep: None,
                    asm_body_key: Some(key),
                }
            }
        };
        if body.native_ref.is_none() {
            body.native_ref = Some(record.id());
        }
        let row_id = record.id();
        body_keys.push(record);
        let Some(visible) = body.visible else {
            continue;
        };
        let suffix = assign(
            native
                .body_visibilities
                .iter()
                .find(|visibility| visibility.body.as_str() == row_id.as_str())
                .map(|row| row.entity_suffix),
            key,
            &reserved_suffixes,
            &mut used_suffixes,
            u64::MAX - 1,
            "Design body entity suffix",
        )?;
        body_visibilities.push(
            BodyVisibility::try_from(BodyVisibilityContent {
                id: format!("f3d:generated:body-visibility#{key}"),
                body: cadmpeg_asm::brep::records::identity::NativeRowId::from(row_id),
                stream: "generated/Design1/BulkStream.dat".into(),
                byte_offset: None,
                asm_body_key_offset: None,
                asm_body_key: key,
                entity_suffix: suffix,
                visible,
            })
            .map_err(CodecError::Malformed)?,
        );
    }
    native.body_native_keys = body_keys;
    native.body_visibilities = body_visibilities;
    Ok(())
}

fn assign(
    preferred: Option<u64>,
    fallback: u64,
    reserved: &BTreeSet<u64>,
    used: &mut BTreeSet<u64>,
    maximum: u64,
    label: &str,
) -> Result<u64, CodecError> {
    if let Some(value) = preferred.filter(|value| *value <= maximum) {
        if used.insert(value) {
            return Ok(value);
        }
    }
    if fallback <= maximum && !reserved.contains(&fallback) && used.insert(fallback) {
        return Ok(fallback);
    }
    let mut value = 1;
    while reserved.contains(&value) || used.contains(&value) {
        value = value
            .checked_add(1)
            .filter(|value| *value <= maximum)
            .ok_or_else(|| CodecError::InvalidInput(format!("F3D {label} capacity exhausted")))?;
    }
    used.insert(value);
    Ok(value)
}
