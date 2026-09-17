// SPDX-License-Identifier: Apache-2.0
//! Parse parameter scopes and exact feature-construction frames.

pub(crate) mod assembly_alignment;
mod assembly_carrier_paths;
mod assembly_operand_frames;
mod assembly_operand_paths;
pub(crate) mod axial_assembly;
pub(crate) mod base_feature;
mod coil;
pub(crate) mod combine;
pub(crate) mod component_constructions;
mod copy_paste_bodies;
pub(crate) mod direct_face;
pub(crate) mod draft;
pub(crate) mod extrude;
pub(crate) mod fixed_parameters;
pub(crate) mod hole;
pub(crate) mod legacy_class_397;
pub(crate) mod legacy_class_415;
mod legacy_operand_paths;
pub(crate) mod mirror;
pub(crate) mod parameter_scope;
pub(crate) mod path_feature;
pub(crate) mod pattern;
pub(crate) mod point_data;
pub(crate) mod shared_frames;
pub(crate) mod sheet_metal;
pub(crate) mod solid_primitive;
pub(crate) mod surfaces;
mod thicken_shell;
pub(crate) mod thread;
pub(crate) mod work_geometry;

#[cfg(test)]
mod tests;
