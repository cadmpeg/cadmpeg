// SPDX-License-Identifier: Apache-2.0
//! Application kernels are emitted from current typed records and neutral edits.

mod meshes;

use super::xml::{self, Object};
use crate::application_geometry::parts::{kernels_for_property, KernelProperty};
use crate::native::entry_contents::{EntryContents, MeshFacet};
use crate::native::PropertyRecord;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{NativeDisposition, NativeDispositions};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, HashMap, HashSet};

pub(super) fn write(
    ir: &CadIr,
    objects: &mut [Object],
    names: &HashMap<String, String>,
    properties: &[PropertyRecord],
    entries: &mut BTreeMap<String, Vec<u8>>,
    outcomes: &mut NativeDispositions,
    losses: &mut Vec<LossNote>,
) -> Result<HashSet<String>, CodecError> {
    let mut emitted_meshes = HashSet::new();
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(emitted_meshes);
    };
    let archive_entries = namespace.arena_as::<crate::native::EntryRecord>("entries")?;
    for property in properties {
        for (entry, value) in kernels_for_property(&archive_entries, KernelProperty::from(property))
        {
            let Some(name) = names.get(&property.owner) else {
                continue;
            };
            let Some(object) = objects.iter_mut().find(|object| &object.name == name) else {
                continue;
            };
            if !ir.scope.model && unavailable_native_mesh(value) {
                omission(losses, &property.id, "all mesh row values are unavailable and there is no current neutral mesh to regenerate");
                object.properties.remove(&property.name);
                continue;
            }
            if ir.scope.model && matches!(value, EntryContents::Mesh { .. }) {
                let occupied = object.properties.keys().cloned().collect();
                let outputs = meshes::write(
                    ir,
                    meshes::MeshRequest {
                        property: &property.id,
                        name: &property.name,
                        object: Some(&property.owner),
                        occupied: &occupied,
                        entry,
                        value,
                    },
                    entries,
                    losses,
                )?;
                if outputs.is_empty() {
                    object.properties.remove(&property.name);
                }
                for output in outputs {
                    let mut target = property.clone();
                    target.name = output.name;
                    if target.name != property.name {
                        target.id =
                            crate::native::native_child_id("property", &target.owner, &target.name);
                        target.status = None;
                        target.dynamic = Some(crate::native::DynamicPropertyMeta {
                            group: "Cadmpeg".into(),
                            documentation: None,
                            attributes: None,
                            read_only: None,
                            hidden: None,
                        });
                        target.body = Some(crate::native::PropertyBody::Persisted {
                            values: mesh_values(&output.file),
                            links: Vec::new(),
                            side_entries: vec![output.file.clone()],
                        });
                    } else {
                        set_app_file(&mut target, "Mesh", &output.file);
                    }
                    if let Some(xml) = xml::native_named_payloads(
                        &target,
                        names,
                        &std::collections::BTreeSet::from([output.file.clone()]),
                    )? {
                        object.set(&target.name, xml);
                        object.mark_geometry_written();
                    }
                    if let Some(mesh) = output.mesh {
                        emitted_meshes.insert(mesh);
                    }
                    super::dispositions::record(
                        outcomes,
                        "entries",
                        entry.id(),
                        if output.regenerated {
                            NativeDisposition::Regenerated
                        } else {
                            NativeDisposition::Written
                        },
                    );
                }
                continue;
            }
            let aliases = point_aliases(ir, entry.id(), Some(&property.owner))?;
            if ir.scope.model && matches!(value, EntryContents::Points { .. }) {
                if let Some(aliases) = &aliases {
                    if aliases.selected != property.id {
                        object.properties.remove(&property.name);
                        continue;
                    }
                    association_reduced(losses, aliases);
                }
            }
            let mut file = entry.name().to_owned();
            let mut preserve_matrix = !ir.scope.model;
            let mut regenerated = match value {
                EntryContents::Mesh {
                    vertices,
                    facets,
                    bounds,
                    ..
                } => {
                    vertices.iter().any(Option::is_none)
                        || facets.iter().any(Option::is_none)
                        || bounds.is_none()
                }
                EntryContents::Points { points } => points.iter().any(Option::is_none),
                _ => false,
            };
            let mut mesh_id = None;
            let (bytes, tag) = match value {
                EntryContents::Mesh {
                    big_endian,
                    vertices,
                    facets,
                    bounds,
                } => {
                    let (mut points, mut triangles) = compact_mesh(vertices, facets)?;
                    let mut bounds = *bounds;
                    if ir.scope.model {
                        let Some(mesh) = ir.model.tessellations.iter().find(|mesh| {
                            mesh.native_ref.as_deref() == Some(entry.id())
                                && belongs(mesh.source_object.as_ref(), Some(&property.owner))
                        }) else {
                            continue;
                        };
                        mesh_id = Some(mesh.id.as_str().to_owned());
                        let current = mesh.vertices();
                        let current_triangles = mesh.triangles();
                        if current != points
                            || current_triangles
                                != triangles
                                    .iter()
                                    .map(|facet| facet.points)
                                    .collect::<Vec<_>>()
                        {
                            points = current;
                            triangles = neighbours(&current_triangles)?;
                            bounds = None;
                            regenerated = true;
                        }
                    }
                    if vertices.iter().any(Option::is_none)
                        || facets.iter().any(Option::is_none)
                        || bounds.is_none()
                    {
                        omission(
                            losses,
                            &property.id,
                            "missing mesh fields are regenerated from the surviving geometry",
                        );
                    }
                    if !precision(&points, &property.id, losses) {
                        continue;
                    }
                    (
                        mesh_bytes(*big_endian, &points, &triangles, bounds)?,
                        "Mesh",
                    )
                }
                EntryContents::Points { points } => {
                    let current = if ir.scope.model {
                        let current = current_points(ir, entry.id(), Some(&property.owner));
                        if current.is_empty() && points.iter().any(Option::is_some) {
                            object.properties.remove(&property.name);
                            continue;
                        }
                        let expected = transformed(property.values(), points)?;
                        preserve_matrix = current == expected;
                        regenerated |= !preserve_matrix;
                        if preserve_matrix {
                            points.iter().flatten().copied().collect()
                        } else {
                            if entry.referenced_by().len() > 1 {
                                file = fresh_member(
                                    &property.id,
                                    entries,
                                    &archive_entries,
                                    "points",
                                )?;
                            }
                            current
                        }
                    } else {
                        points.iter().flatten().copied().collect()
                    };
                    if points.iter().any(Option::is_none) {
                        omission(
                            losses,
                            &property.id,
                            "missing point slots are omitted from the emitted kernel",
                        );
                    }
                    if !precision(&current, &property.id, losses) {
                        continue;
                    }
                    (point_bytes(&current)?, "Points")
                }
                _ => continue,
            };
            let mut output = property.clone();
            if let Some(crate::native::PropertyBody::Persisted {
                values,
                side_entries,
                ..
            }) = &mut output.body
            {
                for value in values
                    .iter_mut()
                    .filter(|value| value.parent.is_none() && value.tag == tag)
                {
                    value.attributes.insert("file".into(), file.clone());
                    if tag == "Points" && !preserve_matrix {
                        value.attributes.remove("mtrx");
                    }
                }
                *side_entries = vec![file.clone()];
            }
            if let Some(value) = xml::native_named_payloads(
                &output,
                names,
                &std::collections::BTreeSet::from([file.clone()]),
            )? {
                object.set(&output.name, value);
            }
            object.mark_geometry_written();
            if entries
                .get(&file)
                .is_some_and(|existing| existing != &bytes)
            {
                file = fresh_member(&property.id, entries, &archive_entries, "kernel")?;
                regenerated = true;
                if let Some(crate::native::PropertyBody::Persisted {
                    values,
                    side_entries,
                    ..
                }) = &mut output.body
                {
                    for value in values
                        .iter_mut()
                        .filter(|value| value.parent.is_none() && value.tag == tag)
                    {
                        value.attributes.insert("file".into(), file.clone());
                    }
                    *side_entries = vec![file.clone()];
                }
                if let Some(value) = xml::native_named_payloads(
                    &output,
                    names,
                    &std::collections::BTreeSet::from([file.clone()]),
                )? {
                    object.set(&output.name, value);
                    object.mark_geometry_written();
                }
            }
            entries.insert(file, bytes);
            if let Some(id) = mesh_id {
                emitted_meshes.insert(id);
            }
            super::dispositions::record(
                outcomes,
                "entries",
                entry.id(),
                if regenerated {
                    NativeDisposition::Regenerated
                } else {
                    NativeDisposition::Written
                },
            );
        }
    }
    Ok(emitted_meshes)
}

/// Regenerate a real GUI-owned kernel without requiring an application object.
pub(super) fn gui_kernel_deleted(
    ir: &CadIr,
    property: &crate::native::GuiPropertyRecord,
) -> Result<bool, CodecError> {
    if !ir.scope.model
        || !matches!(
            property.type_name.as_str(),
            "Mesh::PropertyMeshKernel" | "Points::PropertyPointKernel"
        )
    {
        return Ok(false);
    }
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(false);
    };
    let entries = namespace.arena_as::<crate::native::EntryRecord>("entries")?;
    let object = gui_object(ir, &property.owner)?;
    if property.type_name == "Mesh::PropertyMeshKernel" {
        for (entry, _) in kernels_for_property(&entries, KernelProperty::from(property)) {
            if meshes::omits_carrier(ir, &property.id, entry.id(), object.as_deref())? {
                return Ok(true);
            }
        }
    }
    if property.type_name == "Points::PropertyPointKernel" {
        for (entry, _) in kernels_for_property(&entries, KernelProperty::from(property)) {
            if point_aliases(ir, entry.id(), object.as_deref())?
                .is_some_and(|aliases| aliases.selected != property.id)
            {
                return Ok(true);
            }
        }
    }
    Ok(
        kernels_for_property(&entries, KernelProperty::from(property))
            .into_iter()
            .any(|(entry, value)| match value {
                EntryContents::Mesh {
                    vertices, facets, ..
                } => {
                    (vertices.iter().any(Option::is_some) || facets.iter().any(Option::is_some))
                        && !ir.model.tessellations.iter().any(|mesh| {
                            mesh.native_ref.as_deref() == Some(entry.id())
                                && belongs(mesh.source_object.as_ref(), object.as_deref())
                        })
                }
                EntryContents::Points { points } => {
                    points.iter().any(Option::is_some)
                        && !ir.model.points.iter().any(|point| {
                            point.native_ref.as_deref() == Some(entry.id())
                                && belongs(point.source_object.as_ref(), object.as_deref())
                        })
                }
                _ => false,
            }),
    )
}

#[derive(Default)]
pub(super) struct GuiKernelOutput {
    pub(super) members: std::collections::BTreeSet<String>,
    pub(super) additional: Vec<crate::native::GuiPropertyRecord>,
    pub(super) meshes: HashSet<String>,
}

pub(super) fn write_gui_property(
    ir: &CadIr,
    property: &mut crate::native::GuiPropertyRecord,
    entries: &mut BTreeMap<String, Vec<u8>>,
    outcomes: &mut NativeDispositions,
    losses: &mut Vec<LossNote>,
    has_geometry: &mut bool,
) -> Result<GuiKernelOutput, CodecError> {
    let mut output = GuiKernelOutput::default();
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(output);
    };
    let archive_entries = namespace.arena_as::<crate::native::EntryRecord>("entries")?;
    let object = gui_object(ir, &property.owner)?;
    for (entry, value) in kernels_for_property(&archive_entries, KernelProperty::from(&*property)) {
        if !ir.scope.model && unavailable_native_mesh(value) {
            omission(losses, &property.id, "all mesh row values are unavailable and there is no current neutral mesh to regenerate");
            continue;
        }
        if ir.scope.model && matches!(value, EntryContents::Mesh { .. }) {
            let occupied = namespace
                .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")?
                .iter()
                .filter(|record| record.owner == property.owner)
                .map(|record| record.name.clone())
                .collect();
            let meshes = meshes::write(
                ir,
                meshes::MeshRequest {
                    property: &property.id,
                    name: &property.name,
                    object: object.as_deref(),
                    occupied: &occupied,
                    entry,
                    value,
                },
                entries,
                losses,
            )?;
            for (ordinal, mesh) in meshes.into_iter().enumerate() {
                let mut target = property.clone();
                target.name = mesh.name;
                for value in target
                    .values
                    .iter_mut()
                    .filter(|value| value.parent.is_none() && value.tag == "Mesh")
                {
                    value.attributes.insert("file".into(), mesh.file.clone());
                }
                target.side_entries = vec![mesh.file.clone()];
                output.members.insert(mesh.file);
                if let Some(id) = mesh.mesh {
                    output.meshes.insert(id);
                    *has_geometry = true;
                }
                if ordinal == 0 {
                    *property = target;
                } else {
                    target.id =
                        crate::native::native_child_id("gui-property", &target.owner, &target.name);
                    target.status = None;
                    target.values = mesh_values(&target.side_entries[0]);
                    output.additional.push(target);
                }
                super::dispositions::record(
                    outcomes,
                    "entries",
                    entry.id(),
                    if mesh.regenerated {
                        NativeDisposition::Regenerated
                    } else {
                        NativeDisposition::Written
                    },
                );
            }
            continue;
        }
        if ir.scope.model && matches!(value, EntryContents::Points { .. }) {
            if let Some(aliases) = point_aliases(ir, entry.id(), object.as_deref())? {
                if aliases.selected != property.id {
                    continue;
                }
                association_reduced(losses, &aliases);
            }
        }
        let mut file = entry.name().to_owned();
        let mut regenerated = match value {
            EntryContents::Mesh {
                vertices,
                facets,
                bounds,
                ..
            } => {
                vertices.iter().any(Option::is_none)
                    || facets.iter().any(Option::is_none)
                    || bounds.is_none()
            }
            EntryContents::Points { points } => points.iter().any(Option::is_none),
            _ => false,
        };
        let (bytes, record_has_geometry) = match value {
            EntryContents::Mesh {
                big_endian,
                vertices,
                facets,
                bounds,
            } => {
                let (mut points, mut triangles) = compact_mesh(vertices, facets)?;
                let mut bounds = *bounds;
                if let Some(mesh) = ir.model.tessellations.iter().find(|mesh| {
                    mesh.native_ref.as_deref() == Some(entry.id())
                        && belongs(mesh.source_object.as_ref(), object.as_deref())
                }) {
                    let current = mesh.vertices();
                    let current_triangles = mesh.triangles();
                    if current != points
                        || current_triangles
                            != triangles
                                .iter()
                                .map(|facet| facet.points)
                                .collect::<Vec<_>>()
                    {
                        points = current;
                        triangles = neighbours(&current_triangles)?;
                        bounds = None;
                        regenerated = true;
                    }
                }
                if vertices.iter().any(Option::is_none)
                    || facets.iter().any(Option::is_none)
                    || bounds.is_none()
                {
                    omission(
                        losses,
                        &property.id,
                        "missing mesh fields are regenerated from the surviving geometry",
                    );
                }
                if !precision(&points, &property.id, losses) {
                    continue;
                }
                (
                    mesh_bytes(*big_endian, &points, &triangles, bounds)?,
                    !points.is_empty(),
                )
            }
            EntryContents::Points { points } => {
                let current = if ir.scope.model {
                    let current = current_points(ir, entry.id(), object.as_deref());
                    let unchanged = current == transformed(&property.values, points)?;
                    if unchanged {
                        points.iter().flatten().copied().collect()
                    } else {
                        regenerated = true;
                        if entry.referenced_by().len() > 1 {
                            file = fresh_member(&property.id, entries, &archive_entries, "points")?;
                        }
                        for value in property
                            .values
                            .iter_mut()
                            .filter(|value| value.tag == "Points")
                        {
                            value.attributes.remove("mtrx");
                        }
                        current
                    }
                } else {
                    points.iter().flatten().copied().collect()
                };
                if points.iter().any(Option::is_none) {
                    omission(
                        losses,
                        &property.id,
                        "missing point slots are omitted from the emitted kernel",
                    );
                }
                if !precision(&current, &property.id, losses) {
                    continue;
                }
                (point_bytes(&current)?, !current.is_empty())
            }
            _ => continue,
        };
        if entries
            .get(&file)
            .is_some_and(|existing| existing != &bytes)
        {
            file = fresh_member(&property.id, entries, &archive_entries, "kernel")?;
            regenerated = true;
        }
        for value in property.values.iter_mut().filter(|value| {
            value.parent.is_none() && matches!(value.tag.as_str(), "Mesh" | "Points")
        }) {
            value.attributes.insert("file".into(), file.clone());
        }
        property.side_entries = vec![file.clone()];
        entries.insert(file.clone(), bytes);
        *has_geometry |= record_has_geometry;
        output.members.insert(file);
        super::dispositions::record(
            outcomes,
            "entries",
            entry.id(),
            if regenerated {
                NativeDisposition::Regenerated
            } else {
                NativeDisposition::Written
            },
        );
    }
    Ok(output)
}

fn unavailable_native_mesh(value: &EntryContents) -> bool {
    matches!(value, EntryContents::Mesh { vertices, facets, .. }
        if (!vertices.is_empty() || !facets.is_empty())
            && vertices.iter().all(Option::is_none) && facets.iter().all(Option::is_none))
}

fn mesh_values(file: &str) -> crate::native::ValueSlots {
    vec![crate::native::ValueRecord {
        order: 0,
        parent: None,
        tag: "Mesh".into(),
        attributes: BTreeMap::from([("file".into(), file.to_owned())]),
        text: None,
        decoded_json: None,
        link_index: None,
        item_index: None,
    }]
    .into()
}

fn set_app_file(property: &mut PropertyRecord, tag: &str, file: &str) {
    if let Some(crate::native::PropertyBody::Persisted {
        values,
        side_entries,
        ..
    }) = &mut property.body
    {
        for value in values
            .iter_mut()
            .filter(|value| value.parent.is_none() && value.tag == tag)
        {
            value.attributes.insert("file".into(), file.to_owned());
        }
        *side_entries = vec![file.to_owned()];
    }
}

struct PointAliases {
    selected: String,
    omitted: Vec<String>,
}

fn carriers(
    ir: &CadIr,
    entry_id: &str,
    object: Option<&str>,
    kind: &str,
    tag: &str,
) -> Result<Vec<(bool, usize, String, String)>, CodecError> {
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(Vec::new());
    };
    let entries = namespace.arena_as::<crate::native::EntryRecord>("entries")?;
    let mut carriers = Vec::new();
    for property in namespace.arena_as::<PropertyRecord>("properties")? {
        if property.type_name == kind
            && object == Some(property.owner.as_str())
            && kernels_for_property(&entries, KernelProperty::from(&property))
                .iter()
                .any(|(entry, contents)| {
                    entry.id() == entry_id
                        && matches!(
                            (tag, contents),
                            ("Points", EntryContents::Points { .. })
                                | ("Mesh", EntryContents::Mesh { .. })
                        )
                })
        {
            carriers.push((
                property.name != tag,
                property.order,
                property.name,
                property.id,
            ));
        }
    }
    for property in namespace.arena_as::<crate::native::GuiPropertyRecord>("gui_properties")? {
        if property.type_name == kind
            && gui_object(ir, &property.owner)?.as_deref() == object
            && kernels_for_property(&entries, KernelProperty::from(&property))
                .iter()
                .any(|(entry, contents)| {
                    entry.id() == entry_id
                        && matches!(
                            (tag, contents),
                            ("Points", EntryContents::Points { .. })
                                | ("Mesh", EntryContents::Mesh { .. })
                        )
                })
        {
            carriers.push((
                property.name != tag,
                property.order,
                property.name,
                property.id,
            ));
        }
    }
    carriers.sort();
    Ok(carriers)
}

fn point_aliases(
    ir: &CadIr,
    entry_id: &str,
    object: Option<&str>,
) -> Result<Option<PointAliases>, CodecError> {
    let carriers = carriers(
        ir,
        entry_id,
        object,
        "Points::PropertyPointKernel",
        "Points",
    )?;
    if carriers.len() < 2 {
        return Ok(None);
    }
    let mut carriers = carriers.into_iter();
    let selected = carriers
        .next()
        .ok_or_else(|| CodecError::malformed("kernel alias population is empty"))?
        .3;
    Ok(Some(PointAliases {
        selected,
        omitted: carriers.map(|carrier| carrier.3).collect(),
    }))
}

fn association_reduced(losses: &mut Vec<LossNote>, aliases: &PointAliases) {
    losses.push(crate::loss::FreecadLossCode::ExportKernelAssociationReduced.note(format!(
        "property {} carries the current owner's point union; kernel aliases {} are omitted because individual property associations are not represented",
        aliases.selected, aliases.omitted.join(", ")
    )));
}

fn belongs(source: Option<&cadmpeg_ir::SourceObjectAssociation>, object: Option<&str>) -> bool {
    match (source, object) {
        (Some(source), Some(object)) => {
            source.format == cadmpeg_ir::CodecFormat::Fcstd && source.object_id.as_str() == object
        }
        (None, None) => true,
        _ => false,
    }
}

fn current_points(ir: &CadIr, entry: &str, object: Option<&str>) -> Vec<FinitePoint3> {
    ir.model
        .points
        .iter()
        .filter(|point| {
            point.native_ref.as_deref() == Some(entry)
                && belongs(point.source_object.as_ref(), object)
        })
        .map(|point| point.position())
        .collect()
}

fn transformed(
    values: &[crate::native::ValueRecord],
    points: &[Option<FinitePoint3>],
) -> Result<Vec<FinitePoint3>, CodecError> {
    let matrix = crate::application_geometry::point_transform(values)?;
    points
        .iter()
        .flatten()
        .map(|point| crate::application_geometry::transform_point(matrix, *point))
        .collect()
}

fn fresh_member(
    property: &str,
    entries: &BTreeMap<String, Vec<u8>>,
    original: &[crate::native::EntryRecord],
    suffix: &str,
) -> Result<String, CodecError> {
    let stem = cadmpeg_ir::ids::IdentityKey::encode_segment(property);
    let mut ordinal = 0_u64;
    loop {
        let name = format!("{stem}.{ordinal}.{suffix}");
        if !entries.contains_key(&name) && original.iter().all(|entry| entry.name() != name) {
            return Ok(name);
        }
        ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| CodecError::malformed("kernel member ordinal overflows"))?;
    }
}

fn gui_object(ir: &CadIr, owner: &str) -> Result<Option<String>, CodecError> {
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(None);
    };
    let providers =
        namespace.arena_as::<crate::native::GuiViewProviderRecord>("gui_view_providers")?;
    let extensions = namespace.arena_as::<crate::native::ExtensionRecord>("extensions")?;
    let mut owner = owner;
    for _ in 0..=extensions.len() {
        if let Some(provider) = providers.iter().find(|provider| provider.id == owner) {
            return Ok(provider
                .object
                .as_ref()
                .map(|object| object.as_str().to_owned()));
        }
        let Some(extension) = extensions.iter().find(|extension| extension.id == owner) else {
            return Ok(None);
        };
        owner = &extension.owner;
    }
    Err(CodecError::malformed("GUI kernel owner chain is cyclic"))
}

fn precision(points: &[FinitePoint3], property: &str, losses: &mut Vec<LossNote>) -> bool {
    let mut rounded = false;
    for value in points
        .iter()
        .flat_map(|point| <[f64; 3]>::from(point.get()))
    {
        let Some(narrowed) = cadmpeg_core::convert::f32_from_f64(value) else {
            losses.push(
                crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                    "property {} has a coordinate outside the float32 range; its kernel is omitted",
                    property
                )),
            );
            return false;
        };
        rounded |= f64::from(narrowed) != value;
    }
    if rounded {
        losses.push(
            crate::loss::FreecadLossCode::ExportCoordinateQuantized.note(format!(
                "property {} coordinates are rounded to the kernel float32 representation",
                property
            )),
        );
    }
    true
}

fn omission(losses: &mut Vec<LossNote>, property: &str, detail: &str) {
    losses.push(
        cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted
            .note(format!("property {}: {detail}", property)),
    );
}

fn compact_mesh(
    vertices: &[Option<FinitePoint3>],
    facets: &[Option<MeshFacet>],
) -> Result<(Vec<FinitePoint3>, Vec<MeshFacet>), CodecError> {
    let mut points = Vec::new();
    let mut indices = BTreeMap::new();
    for (index, point) in vertices.iter().enumerate() {
        if let Some(point) = point {
            indices.insert(
                u32::try_from(index)
                    .map_err(|_| CodecError::malformed("mesh source vertex index exceeds u32"))?,
                count(points.len())?,
            );
            points.push(*point);
        }
    }
    let mut kept = Vec::new();
    let mut facet_indices = BTreeMap::new();
    for (index, facet) in facets.iter().enumerate() {
        let Some(facet) = facet else {
            continue;
        };
        let [Some(a), Some(b), Some(c)] = facet.points.map(|index| indices.get(&index).copied())
        else {
            continue;
        };
        facet_indices.insert(count(index)?, count(kept.len())?);
        kept.push(MeshFacet {
            points: [a, b, c],
            neighbours: facet.neighbours,
        });
    }
    for facet in &mut kept {
        facet.neighbours = facet
            .neighbours
            .map(|index| index.and_then(|index| facet_indices.get(&index).copied()));
    }
    Ok((points, kept))
}

fn neighbours(triangles: &[[u32; 3]]) -> Result<Vec<MeshFacet>, CodecError> {
    let mut facets = triangles
        .iter()
        .map(|points| MeshFacet {
            points: *points,
            neighbours: [None; 3],
        })
        .collect::<Vec<_>>();
    let mut edges = BTreeMap::<(u32, u32), Vec<(usize, usize)>>::new();
    for (index, points) in triangles.iter().enumerate() {
        for (slot, (a, b)) in [
            (points[0], points[1]),
            (points[1], points[2]),
            (points[2], points[0]),
        ]
        .into_iter()
        .enumerate()
        {
            let mut edge = [a, b];
            edge.sort_unstable();
            edges
                .entry((edge[0], edge[1]))
                .or_default()
                .push((index, slot));
        }
    }
    for incident in edges.values() {
        if let [(a, sa), (b, sb)] = incident.as_slice() {
            facets[*a].neighbours[*sa] = Some(count(*b)?);
            facets[*b].neighbours[*sb] = Some(count(*a)?);
        }
    }
    Ok(facets)
}

fn count(value: usize) -> Result<u32, CodecError> {
    u32::try_from(value)
        .map_err(|_| CodecError::malformed("application kernel population exceeds u32"))
}

fn word(bytes: &mut Vec<u8>, value: u32, big: bool) {
    bytes.extend_from_slice(&if big {
        value.to_be_bytes()
    } else {
        value.to_le_bytes()
    });
}

fn coordinate(bytes: &mut Vec<u8>, value: f64, big: bool) -> Result<f32, CodecError> {
    let value = cadmpeg_core::convert::f32_from_f64(value).ok_or_else(|| {
        CodecError::malformed("application coordinate cannot be represented as float32")
    })?;
    bytes.extend_from_slice(&if big {
        value.to_be_bytes()
    } else {
        value.to_le_bytes()
    });
    Ok(value)
}

fn point_bytes(points: &[FinitePoint3]) -> Result<Vec<u8>, CodecError> {
    let mut bytes = Vec::new();
    word(&mut bytes, count(points.len())?, false);
    for point in points {
        for value in <[f64; 3]>::from(point.get()) {
            coordinate(&mut bytes, value, false)?;
        }
    }
    Ok(bytes)
}

fn mesh_bytes(
    big: bool,
    points: &[FinitePoint3],
    facets: &[MeshFacet],
    bounds: Option<[cadmpeg_ir::scalar::FiniteBinary32; 6]>,
) -> Result<Vec<u8>, CodecError> {
    let mut bytes = Vec::new();
    word(
        &mut bytes,
        crate::layout::mesh_kernel_side_entry_header::MAGIC_VALUE,
        big,
    );
    word(
        &mut bytes,
        crate::layout::mesh_kernel_side_entry_header::VERSION_VALUE,
        big,
    );
    bytes.resize(crate::layout::mesh_kernel_side_entry_header::LEN, 0);
    word(&mut bytes, count(points.len())?, big);
    word(&mut bytes, count(facets.len())?, big);
    for point in points {
        for value in <[f64; 3]>::from(point.get()) {
            coordinate(&mut bytes, value, big)?;
        }
    }
    for facet in facets {
        for value in facet.points {
            word(&mut bytes, value, big);
        }
        for neighbour in facet.neighbours {
            match neighbour {
                Some(index) => word(&mut bytes, index, big),
                None => word(&mut bytes, u32::MAX, big),
            }
        }
    }
    let bounds = bounds
        .map(|values| values.map(|value| f64::from(value.get())))
        .unwrap_or_else(|| {
            let mut result = [0.0; 6];
            if let Some(first) = points.first() {
                let first = <[f64; 3]>::from(first.get());
                result = [first[0], first[0], first[1], first[1], first[2], first[2]];
                for point in points {
                    for (axis, value) in <[f64; 3]>::from(point.get()).into_iter().enumerate() {
                        result[axis] = result[axis].min(value);
                        result[axis + 3] = result[axis + 3].max(value);
                    }
                }
            }
            result
        });
    for value in bounds {
        coordinate(&mut bytes, value, big)?;
    }
    Ok(bytes)
}
