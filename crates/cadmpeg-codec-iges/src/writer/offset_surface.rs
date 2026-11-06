// SPDX-License-Identifier: Apache-2.0
//! Write exact offsets of canonical planes.

use super::{
    number, reference_marker, CadIr, Entity, EntityStatus, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::geometry::{OffsetExtension, ProceduralSurfaceDefinition};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::SourceGeometryRole;

fn candidate<'a>(ir: &'a CadIr, surface: &Surface) -> Option<(&'a Surface, f64)> {
    let construction = surface.geometry.procedural_construction()?;
    let procedural = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|row| row.id == *construction)?;
    let ProceduralSurfaceDefinition::Offset(offset) = procedural.definition() else {
        return None;
    };
    if *offset.u_sense() != Some(0)
        || *offset.v_sense() != Some(0)
        || offset.linear_support_extension()
        || !matches!(
            offset.extension(),
            OffsetExtension::Legacy {
                flags: cadmpeg_ir::geometry::LegacyExtensionFlags::Absent {},
                cache: None
            }
        )
    {
        return None;
    }
    let support = ir
        .model
        .surfaces
        .iter()
        .find(|row| row.id == *offset.support())?;
    if support.source_object.as_ref().and_then(|source| source.geometry_role) != Some(SourceGeometryRole::Support)
        || ir.model.faces.iter().any(|face| face.surface == support.id)
        || ir.model.procedural_surfaces.iter().filter(|row| matches!(row.definition(), ProceduralSurfaceDefinition::Offset(other) if other.support() == &support.id)).count() != 1
    { return None; }
    let (
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(base)),
        Some(SolvedSurfaceGeometry::Plane(result)),
    ) = (&support.geometry, surface.geometry.solved())
    else {
        return None;
    };
    let origin = base.origin().get();
    let distance = offset.distance().get();
    if distance == 0.0
        || origin.x != 0.0
        || origin.y != 0.0
        || *base.frame().axis().as_raw() != Vector3::new(0.0, 0.0, 1.0)
        || *base.frame().reference().as_raw() != Vector3::new(1.0, 0.0, 0.0)
        || result.frame() != base.frame()
        || result.origin().get() != origin.translated(Vector3::new(0.0, 0.0, 1.0), distance)
    {
        return None;
    }
    Some((support, distance))
}

pub(super) fn consumed_support(ir: &CadIr, surface: &Surface, version: crate::IgesVersion) -> bool {
    let ownership = super::ownership::SourceOwnership::build(ir);
    ir.model.surfaces.iter().any(|owner| {
        ownership.surface_status(owner).is_some()
            && candidate(ir, owner).is_some_and(|(support, _)| support.id == surface.id)
            && admitted(version)
    })
}

fn admitted(version: crate::IgesVersion) -> bool {
    crate::profile::envelope_a_admits(
        140,
        0,
        crate::version::VersionFlag::from_write_version(version).global_table(),
    )
}

pub(super) fn entities(
    ir: &CadIr,
    surface: &Surface,
    base_index: usize,
    version: crate::IgesVersion,
) -> Option<Vec<Entity>> {
    if !admitted(version) {
        return None;
    }
    let (support, distance) = candidate(ir, surface)?;
    let SolvedSurfaceGeometry::Plane(base) = support.geometry.solved()? else {
        return None;
    };
    let z = number(cadmpeg_ir::scalar::FiniteReal::new(base.origin().get().z)?);
    let support_entity = Entity {
        structure: None,
        property_links: Vec::new(),
        association_links: Vec::new(),
        line_font: None,
        source_targets: vec![support.id.to_string()],
        type_code: 108,
        form: 0,
        label: String::new(),
        status: EntityStatus::PhysicallyDependent,
        subscript: 0,
        parameter_body: format!("0,0,1,{z},0,0,0,{z},0;").into_bytes(),
        transform: None,
        transform_link: None,
    };
    let offset_entity = Entity {
        structure: None,
        property_links: Vec::new(),
        association_links: Vec::new(),
        line_font: None,
        source_targets: Vec::new(),
        type_code: 140,
        form: 0,
        label: String::new(),
        status: EntityStatus::Independent,
        subscript: 0,
        parameter_body: format!(
            "0,0,1,{},{};",
            number(cadmpeg_ir::scalar::FiniteReal::new(distance)?),
            reference_marker(base_index)
        )
        .into_bytes(),
        transform: None,
        transform_link: None,
    };
    Some(vec![support_entity, offset_entity])
}
