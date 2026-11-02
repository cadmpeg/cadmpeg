// SPDX-License-Identifier: Apache-2.0
//! Typed F3D native-namespace helpers for crate tests.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::write::target::TargetRequest;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::Write;

use cadmpeg_ir::codec::write::Encoder;

use crate::F3dCodec;

pub(crate) fn timestamp_attribute(
    ordinal: u32,
    target: cadmpeg_ir::attributes::AttributeTarget,
    value: cadmpeg_ir::scalar::FiniteReal,
) -> cadmpeg_ir::attributes::SourceAttribute {
    use cadmpeg_ir::attributes::{AttributeValue, SourceAttribute};
    SourceAttribute {
        native_ref: None,
        id: cadmpeg_ir::ids::AttributeId::mint(format!("synthetic:timestamp:attribute#{ordinal}"))
            .expect("timestamp attribute identity"),
        target,
        name: "ATTRIB_CUSTOM-attrib".into(),
        values: vec![
            Some(AttributeValue::String("Timestamp_attrib_def".into())),
            Some(AttributeValue::Integer(1)),
            Some(AttributeValue::Float(value)),
        ],
    }
}

pub(crate) fn edit_timestamp_attribute(
    ir: &mut cadmpeg_ir::document::CadIr,
    row: &cadmpeg_asm::brep::records::identity::NativeRowId,
    target_coedge: Option<u32>,
    value: cadmpeg_ir::scalar::FiniteReal,
) {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
    let neutral = crate::brep::link_neutral_id(
        &ir.model.bodies,
        &ir.model.faces,
        &ir.model.edges,
        &ir.model.vertices,
        &ir.model.coedges,
        row,
        target_coedge,
    )
    .expect("linked timestamp target");
    fn target_id(target: &AttributeTarget) -> Option<&str> {
        match target {
            AttributeTarget::Body(id) => Some(id.as_str()),
            AttributeTarget::Face(id) => Some(id.as_str()),
            AttributeTarget::Edge(id) => Some(id.as_str()),
            AttributeTarget::Coedge(id) => Some(id.as_str()),
            AttributeTarget::Vertex(id) => Some(id.as_str()),
            AttributeTarget::Record { id, .. } => Some(id.as_str()),
            _ => None,
        }
    }
    let mut attributes = ir.model.attributes.iter_mut().filter(|attribute| {
        target_id(&attribute.target).as_deref() == Some(neutral.as_str())
            && crate::brep::creation_timestamp_value(attribute).is_some()
    });
    let attribute = attributes.next().expect("canonical timestamp attribute");
    assert!(
        attributes.next().is_none(),
        "one timestamp attribute per target"
    );
    let previous = crate::brep::creation_timestamp_value(attribute).unwrap();
    let mut values = attribute.values.iter_mut().filter_map(|field| match field {
        Some(AttributeValue::Float(current)) if *current == previous => Some(current),
        _ => None,
    });
    let current = values.next().expect("canonical timestamp value");
    assert!(values.next().is_none(), "one timestamp value per attribute");
    *current = value;
}

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
    let text = serde_json::to_string(&stored.arenas()[arena_name][0]).unwrap();
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

/// Rename one unit-cube coedge to the neutral id a tolerant-coedge native
/// record pairs with, fixing loop rings, vertex-use anchors, and radial links.
///
/// Tolerant-coedge parameters pair with their neutral carrier by shared
/// source record: the neutral id is `{scope}brep:entity#{record_index}` for
/// the record's source namespace. Synthetic cube ids carry no numeric record
/// suffix, so the targeted coedge takes the paired identity for `namespace`.
pub(crate) fn pair_cube_coedge_with_native_record(
    ir: &mut cadmpeg_ir::document::CadIr,
    namespace: &cadmpeg_asm::brep::records::identity::NativeRecordNamespace,
    ordinal: usize,
    record_index: u32,
) -> cadmpeg_ir::ids::CoedgeId {
    let paired = cadmpeg_ir::ids::CoedgeId::mint(
        cadmpeg_asm::brep::records::native_entity_id(namespace, record_index)
            .expect("paired coedge identity"),
    )
    .expect("paired coedge identity");
    let previous = std::mem::replace(&mut ir.model.coedges[ordinal].id, paired.clone());
    for coedge in &mut ir.model.coedges {
        if coedge.radial_next == previous {
            coedge.radial_next = paired.clone();
        }
    }
    for loop_ in &mut ir.model.loops {
        let cadmpeg_ir::topology::LoopBoundary::Ring(ring) = loop_.boundary.clone() else {
            continue;
        };
        if !ring.coedges().contains(&previous) {
            continue;
        }
        let coedges = ring
            .coedges()
            .iter()
            .map(|id| {
                if *id == previous {
                    paired.clone()
                } else {
                    id.clone()
                }
            })
            .collect::<Vec<_>>();
        let vertex_uses = ring
            .vertex_uses()
            .iter()
            .map(|use_| {
                let mut use_ = use_.clone();
                if use_.after == previous {
                    use_.after = paired.clone();
                }
                use_
            })
            .collect::<Vec<_>>();
        loop_
            .replace_ring(coedges, vertex_uses)
            .expect("paired coedge ring stays well-formed");
    }
    paired
}

pub(crate) trait TestEncode {
    fn encode(
        &self,
        ir: &cadmpeg_ir::CadIr,
        output: &mut dyn Write,
    ) -> Result<cadmpeg_ir::report::export::ExportReport, cadmpeg_core::CodecError>;
}

impl TestEncode for F3dCodec {
    fn encode(
        &self,
        ir: &cadmpeg_ir::CadIr,
        output: &mut dyn Write,
    ) -> Result<cadmpeg_ir::report::export::ExportReport, cadmpeg_core::CodecError> {
        self.plan(
            cadmpeg_ir::codec::write::EncodeInput::new(ir, None),
            TargetRequest::Inherit,
        )?
        .write_to(output)
    }
}

pub(crate) fn f3d_native(ir: &cadmpeg_ir::document::CadIr) -> crate::native::F3dNative {
    crate::native::F3dNative::load(ir.native.namespace("f3d").expect("F3D native namespace"))
        .unwrap()
}

/// Neutral entity a canonical native row links, resolved through the neutral
/// record's `native_ref`. Coedge targets name their owning edge's row with
/// the coedge source index.
pub(crate) fn native_row_neutral(
    ir: &cadmpeg_ir::document::CadIr,
    row: &cadmpeg_asm::brep::records::identity::NativeRowId,
    target_coedge: Option<u32>,
) -> Option<String> {
    crate::brep::link_neutral_id(
        &ir.model.bodies,
        &ir.model.faces,
        &ir.model.edges,
        &ir.model.vertices,
        &ir.model.coedges,
        row,
        target_coedge,
    )
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
                &cadmpeg_test_support::service_decode_context(),
                self.ir.native.namespace_mut("f3d"),
            )
            .unwrap();
    }
}

pub(crate) fn f3d_native_mut(ir: &mut cadmpeg_ir::document::CadIr) -> F3dNativeMut<'_> {
    let native = ir
        .native
        .namespace("f3d")
        .map(crate::native::F3dNative::load)
        .transpose()
        .unwrap()
        .unwrap_or_default();
    F3dNativeMut { ir, native }
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
    let wire = serde_json::to_value(record).unwrap();
    let restored: T = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(restored).unwrap(), wire);
    for id in alternatives {
        let mut invalid = wire.clone();
        invalid["id"] = serde_json::json!(id);
        let error = serde_json::from_value::<T>(invalid).expect_err("invalid id");
        assert!(error.to_string().contains("id"), "{error}");
    }
}
