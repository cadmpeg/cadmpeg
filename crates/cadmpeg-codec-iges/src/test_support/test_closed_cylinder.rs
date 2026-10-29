// SPDX-License-Identifier: Apache-2.0
//! The shared neutral solid cylinder with a stated support role on each
//! curve.

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::{CadIr, CodecFormat, SourceGeometryRole, SourceObjectAssociation};

/// A curve association that states the curve exists only to carry topology.
fn support(key: &str) -> SourceObjectAssociation {
    SourceObjectAssociation {
        format: CodecFormat::Iges,
        geometry_role: Some(SourceGeometryRole::Support),
        object_id: NonBlankString::new(key).expect("non-blank key"),
        instance_path: Vec::new(),
    }
}

/// [`cadmpeg_test_support::solids::solid_cylinder`] whose bottom, top and
/// seam curves each state the support role.
pub(crate) fn solid_cylinder() -> CadIr {
    cadmpeg_test_support::solids::cylinder_with_curve_sources(|key| Some(support(key)))
}
