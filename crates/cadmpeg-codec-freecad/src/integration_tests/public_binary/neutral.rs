// SPDX-License-Identifier: Apache-2.0
//! Independent semantic expectations from the edited input's typed values.

use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;
use cadmpeg_test_support::framing::freecad::binary::BinaryKind;

mod design;
mod gui;
mod kernel;

pub(super) fn update(
    ir: &mut CadIr,
    entry: &crate::native::EntryRecord,
    kind: BinaryKind,
) -> Result<(), CodecError> {
    match kind {
        BinaryKind::Color | BinaryKind::Material { .. } => gui::update(ir, entry, kind),
        BinaryKind::Mesh | BinaryKind::Points => kernel::update(ir, entry, kind),
        BinaryKind::Float
        | BinaryKind::Vector
        | BinaryKind::Placement
        | BinaryKind::Fillet
        | BinaryKind::F32 { .. }
        | BinaryKind::MeshMaterial => design::update(ir, entry, kind),
    }
}
