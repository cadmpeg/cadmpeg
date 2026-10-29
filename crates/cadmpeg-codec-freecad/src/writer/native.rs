// SPDX-License-Identifier: Apache-2.0
//! Native-only properties that may accompany regenerated neutral objects.

mod attachments;
mod extensions;
mod maps;
mod neutral;
mod operands;
mod topology_join;
pub(super) use attachments::write as attachments;
pub(super) use extensions::write as extensions;
pub(super) use maps::write as maps;
pub(super) use operands::write as operands;
pub(super) use topology_join::TopologyJoin;
pub(super) fn retained(
    ir: &cadmpeg_ir::CadIr,
    object: &crate::native::ObjectRecord,
    properties: &[crate::native::PropertyRecord],
) -> Result<bool, cadmpeg_core::CodecError> {
    neutral::retained(ir, object, properties)
}
pub(super) fn property_held(
    ir: &cadmpeg_ir::CadIr,
    property: &crate::native::PropertyRecord,
    objects: &[crate::native::ObjectRecord],
) -> bool {
    neutral::property_held(ir, property, objects)
}
