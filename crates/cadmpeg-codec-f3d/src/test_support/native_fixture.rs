// SPDX-License-Identifier: Apache-2.0
//! Typed F3D native-namespace helpers for crate tests.
#![allow(clippy::unwrap_used)]

use cadmpeg_core::serde_erase::json;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::Write;

use cadmpeg_ir::codec::write::Encoder;

use crate::F3dCodec;

pub(crate) fn assert_borrowed_native_retained_limit<T: Serialize>(
    record: &T,
    arena_name: &str,
    reset_clone_count: impl FnOnce(),
    clone_count: impl Fn() -> usize,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let arena = DecodeArena::new();
    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut stored = cadmpeg_ir::NativeNamespace::default();
    stored
        .set_arena(&service, arena_name, std::slice::from_ref(record))
        .unwrap();
    assert_eq!(stored.arenas()[arena_name].len(), 1);

    // The stored text is the record's only retained charge, so a ceiling one
    // byte below its length refuses the record.
    let text = json::to_string(&stored.arenas()[arena_name][0]).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(text.len()).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    reset_clone_count();
    let error = namespace
        .set_arena(&limited, arena_name, std::slice::from_ref(record))
        .unwrap_err();
    assert_eq!(clone_count(), 0);
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));
}

pub(crate) trait TestEncode {
    fn encode(
        &self,
        ir: &cadmpeg_ir::CadIr,
        output: &mut dyn Write,
    ) -> Result<cadmpeg_ir::report::export::ExportReport, cadmpeg_ir::codec::write::EncodeError>;
}

impl TestEncode for F3dCodec {
    fn encode(
        &self,
        ir: &cadmpeg_ir::CadIr,
        output: &mut dyn Write,
    ) -> Result<cadmpeg_ir::report::export::ExportReport, cadmpeg_ir::codec::write::EncodeError>
    {
        self.plan(cadmpeg_ir::codec::write::EncodeInput::new(ir, None))?
            .write_to(output)
    }
}

pub(crate) fn f3d_native(ir: &cadmpeg_ir::document::CadIr) -> crate::native::F3dNative {
    crate::native::F3dNative::load_inner(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        ir.native.namespace("f3d").expect("F3D native namespace"),
    )
    .unwrap()
}

pub(crate) struct F3dNativeMut<'a> {
    ir: &'a mut cadmpeg_ir::document::CadIr,
    native: crate::native::F3dNative,
}

impl std::ops::Deref for F3dNativeMut<'_> {
    type Target = crate::native::F3dNative;

    fn deref(&self) -> &Self::Target {
        &self.native
    }
}

impl std::ops::DerefMut for F3dNativeMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.native
    }
}

impl Drop for F3dNativeMut<'_> {
    fn drop(&mut self) {
        self.native
            .store(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                self.ir.native.namespace_mut(cadmpeg_ir::codec_id!("f3d")),
            )
            .unwrap();
    }
}

pub(crate) fn f3d_native_mut(ir: &mut cadmpeg_ir::document::CadIr) -> F3dNativeMut<'_> {
    let native = ir
        .native
        .namespace("f3d")
        .map(|namespace| {
            crate::native::F3dNative::load_inner(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                namespace,
            )
        })
        .transpose()
        .unwrap()
        .unwrap_or_default();
    F3dNativeMut { ir, native }
}

/// Give `ir`, which holds no F3D native namespace, an empty one, as a
/// decoded design with no native record holds.
pub(crate) fn add_empty_f3d_native(ir: &mut cadmpeg_ir::document::CadIr) {
    assert!(ir.native.namespace("f3d").is_none());
    crate::native::F3dNative::default()
        .store(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            ir.native.namespace_mut(cadmpeg_ir::codec_id!("f3d")),
        )
        .unwrap();
}

pub(crate) fn update_f3d_native<R>(
    ir: &mut cadmpeg_ir::document::CadIr,
    update: impl FnOnce(&mut crate::native::F3dNative) -> R,
) -> R {
    let mut native = f3d_native_mut(ir);
    update(&mut native)
}

pub(crate) fn reject_changed_id<T: Serialize + DeserializeOwned + std::fmt::Debug>(
    record: T,
    alternatives: &[&str],
) {
    let wire = json::to_value(record).unwrap();
    let restored: T = json::from_value(wire.clone()).unwrap();
    assert_eq!(json::to_value(restored).unwrap(), wire);
    for id in alternatives {
        let mut invalid = wire.clone();
        invalid["id"] = cadmpeg_core::json!(id);
        let error = json::from_value::<T>(invalid).expect_err("invalid id");
        assert!(error.to_string().contains("id"), "{error}");
    }
}
