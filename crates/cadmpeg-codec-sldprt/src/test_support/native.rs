// SPDX-License-Identifier: Apache-2.0
//! Native-namespace load and store helpers for crate tests.
#![allow(clippy::unwrap_used)]

use crate::resolved_features::class_sources::ClassSources;
pub(crate) fn sldprt_native(ir: &cadmpeg_ir::CadIr) -> crate::native::SldprtNative {
    crate::native::SldprtNative::load(
        ir.native
            .namespace("sldprt")
            .expect("SLDPRT native namespace"),
    )
    .unwrap()
}

pub(crate) fn update_sldprt_native<R>(
    ir: &mut cadmpeg_ir::CadIr,
    update: impl FnOnce(&mut crate::native::SldprtNative) -> R,
) -> R {
    let mut native = sldprt_native(ir);
    let result = update(&mut native);
    native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            ir.native.namespace_mut("sldprt"),
        )
        .unwrap();
    result
}

/// Every pending layout pointer of `sources`: its feature-input pointers and
/// its class pointers, as the decoder installs them.
pub(crate) fn layout_pointers(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    sources: crate::resolved_features::body_sources::BodySources<'_>,
) -> std::collections::BTreeMap<String, std::collections::BTreeMap<String, u64>> {
    let classes = sources.class_sources();
    let mut pointers = sources.into_pointers();
    pointers.extend(classes.into_pointers(ctx).unwrap());
    pointers
}

impl ClassSources {
    /// Class positions that a fixture states for its classes, admitted as
    /// the decoder admits a declaration marker from its lane View. A class id
    /// that repeats must repeat its position.
    pub(crate) fn from_positions<'a>(
        positions: impl IntoIterator<Item = (&'a crate::records::FeatureInputClass, u64)>,
    ) -> Self {
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut sources = Self::default();
        for (class, position) in positions {
            match sources.position(class) {
                Some(stated) => assert_eq!(stated, position, "class {} repeats", class.id),
                None => sources.insert(&ctx, &class.id, position).unwrap(),
            }
        }
        sources
    }
}

/// Admit class positions from the unchanged fixture source, independently of
/// held native edits. Source-capture tests use their original View directly.
pub(crate) fn class_sources<L: std::borrow::Borrow<crate::records::FeatureInputLane>>(
    lanes: &[L],
) -> ClassSources {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut unique = std::collections::BTreeMap::new();
    for lane in lanes {
        let lane = lane.borrow();
        if let Some(previous) = unique.insert(lane.id.as_str(), lane) {
            assert_eq!(
                previous.native_payload.as_slice(),
                lane.native_payload.as_slice(),
                "one source owner has conflicting fixture bytes"
            );
        }
    }
    let bytes = unique
        .values()
        .flat_map(|lane| lane.native_payload.iter().copied())
        .collect::<Vec<_>>();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut positions = ClassSources::default();
    let mut start = root.start();
    for lane in unique.values() {
        let end = start.checked_add(lane.native_payload.len()).unwrap();
        let source = root.child(start, end).unwrap();
        let classes =
            crate::resolved_features::names::class_declarations(source.window(), &lane.id).unwrap();
        positions.admit_source(&ctx, source, &classes).unwrap();
        start = end;
    }
    positions
}
