// SPDX-License-Identifier: Apache-2.0
//! Feature history transfer: dimensions, recipes, result topology, and named definitions.

#[allow(clippy::wildcard_imports)]
use super::*;

pub(super) fn link_feature_sketch_history(scan: &ContainerScan, ir: &mut CadIr) {
    let links = scan
        .features
        .section_transforms
        .iter()
        .filter(|transform| {
            unique_feature_section_transform(
                &scan.features.section_transforms,
                transform.definition_id,
                transform.offset,
            )
            .is_some()
        })
        .filter_map(|transform| {
            let owner = IrFeatureId(format!("creo:model:feature#{}", transform.feature_id?));
            let definition =
                unique_feature_definition_for_transform(&scan.features.definitions, transform)?;
            let sketch = model_sketch_id(scan, definition);
            let sketch_feature = section_owner_feature_id(scan, transform.definition_id, &sketch);
            ir.model
                .features
                .iter()
                .any(|feature| feature.id == sketch_feature)
                .then_some((owner, sketch_feature))
        })
        .collect::<Vec<_>>();
    for (owner, sketch_feature) in links {
        let Some(feature) = ir
            .model
            .features
            .iter_mut()
            .find(|feature| feature.id == owner)
        else {
            continue;
        };
        if !feature.dependencies.contains(&sketch_feature) {
            feature.dependencies.push(sketch_feature);
        }
    }
}

pub(super) fn surface_kind_for_geometry(
    geometry: &SurfaceGeometry,
) -> Option<crate::surface::SurfaceKind> {
    match geometry {
        SurfaceGeometry::Plane { .. } => Some(crate::surface::SurfaceKind::Plane),
        SurfaceGeometry::Cylinder { .. } => Some(crate::surface::SurfaceKind::Cylinder),
        SurfaceGeometry::Cone { .. } => Some(crate::surface::SurfaceKind::Cone),
        SurfaceGeometry::Sphere { .. } | SurfaceGeometry::Torus { .. } => {
            Some(crate::surface::SurfaceKind::TorusOrSphere)
        }
        SurfaceGeometry::Nurbs(_) => Some(crate::surface::SurfaceKind::Spline),
        SurfaceGeometry::Transformed { basis, .. } => surface_kind_for_geometry(basis),
        SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Unknown { .. } => None,
    }
}

pub(super) fn generated_surface_id_for_feature(
    tables: &[crate::feature::FeatureEntityTable],
    feature_id: u32,
    source_entity_id: u32,
) -> Option<u32> {
    let mut matches = tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .flat_map(|table| {
            table
                .entries
                .iter()
                .filter(|entry| {
                    entry.class_id == 200 && entry.source_entity_id == Some(source_entity_id)
                })
                .filter(|entry| table.surface_ids.contains(&entry.entity_id))
                .map(|entry| entry.entity_id)
        });
    let surface_id = matches.next()?;
    matches.next().is_none().then_some(surface_id)
}

pub(super) fn section_entity_is_generated_profile(
    segment_table_complete: bool,
    feature_id: Option<u32>,
    source_entity_id: u32,
    expected_kinds: &[crate::surface::SurfaceKind],
    tables: &[crate::feature::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> bool {
    if !segment_table_complete {
        return false;
    }
    let Some(feature_id) = feature_id else {
        return false;
    };
    let direct = generated_surface_id_for_feature(tables, feature_id, source_entity_id)
        .is_some_and(|surface_id| {
            crate::surface::unique_surface_row(rows, surface_id).is_some_and(|row| {
                row.feature_id == feature_id && expected_kinds.contains(&row.kind)
            })
        });
    if direct {
        return true;
    }
    if !expected_kinds.contains(&crate::surface::SurfaceKind::Cylinder) {
        return false;
    }
    let mut blind_cylinders = tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .filter_map(|table| {
            let [rowless_cap, cap, profile, cylinder] = table.entries.as_slice() else {
                return None;
            };
            ([
                rowless_cap.class_id,
                cap.class_id,
                profile.class_id,
                cylinder.class_id,
            ] == [204, 203, 200, 200]
                && profile.source_entity_id == Some(source_entity_id)
                && cylinder.source_entity_id.is_none()
                && table.surface_ids.contains(&cap.entity_id)
                && table.surface_ids.contains(&cylinder.entity_id)
                && table
                    .non_surface_entity_ids
                    .contains(&rowless_cap.entity_id)
                && table.non_surface_entity_ids.contains(&profile.entity_id)
                && crate::surface::unique_surface_row(rows, cylinder.entity_id).is_some_and(
                    |row| {
                        row.feature_id == feature_id
                            && row.kind == crate::surface::SurfaceKind::Cylinder
                    },
                ))
            .then_some(cylinder.entity_id)
        });
    blind_cylinders.next().is_some() && blind_cylinders.next().is_none()
}

pub(super) fn section_generated_profile_surface_kinds(
    geometry: &SketchGeometry,
) -> Option<&'static [crate::surface::SurfaceKind]> {
    match geometry {
        SketchGeometry::Line { .. } => Some(&[crate::surface::SurfaceKind::Plane]),
        SketchGeometry::Arc { .. } | SketchGeometry::Circle { .. } => {
            Some(&[crate::surface::SurfaceKind::Cylinder])
        }
        SketchGeometry::Nurbs { .. } => Some(&[
            crate::surface::SurfaceKind::Spline,
            crate::surface::SurfaceKind::Extrusion,
        ]),
        _ => None,
    }
}

pub(super) fn ordered_analytic_surface_id_for_feature(
    surface_rows: &[crate::surface::SurfaceRow],
    tables: &[crate::feature::FeatureEntityTable],
    feature_id: u32,
    order: &crate::feature::FeatureOrderTable,
    external_id: u32,
    geometry: &SurfaceGeometry,
) -> Option<u32> {
    order.internal_id(external_id)?;
    analytic_surface_id_for_feature(surface_rows, tables, feature_id, external_id, geometry)
}

pub(super) fn analytic_surface_id_for_feature(
    surface_rows: &[crate::surface::SurfaceRow],
    tables: &[crate::feature::FeatureEntityTable],
    feature_id: u32,
    external_id: u32,
    geometry: &SurfaceGeometry,
) -> Option<u32> {
    let surface_id = generated_surface_id_for_feature(tables, feature_id, external_id)?;
    let expected_kind = surface_kind_for_geometry(geometry)?;
    crate::surface::unique_surface_row(surface_rows, surface_id)
        .is_some_and(|row| row.feature_id == feature_id && row.kind == expected_kind)
        .then_some(surface_id)
}

pub(super) fn ordered_family_surface_bindings_for_feature(
    surface_rows: &[crate::surface::SurfaceRow],
    feature_id: u32,
    tables: &[crate::feature::FeatureEntityTable],
    order: &crate::feature::FeatureOrderTable,
    external_ids: impl IntoIterator<Item = u32>,
    expected_kind: crate::surface::SurfaceKind,
) -> BTreeMap<u32, u32> {
    let mut bindings = BTreeMap::new();
    let mut bound_surfaces = BTreeSet::new();
    for external_id in external_ids {
        if order.internal_id(external_id).is_none() {
            return BTreeMap::new();
        }
        let Some(surface_id) = generated_surface_id_for_feature(tables, feature_id, external_id)
        else {
            return BTreeMap::new();
        };
        if !crate::surface::unique_surface_row(surface_rows, surface_id)
            .is_some_and(|row| row.feature_id == feature_id && row.kind == expected_kind)
            || !bound_surfaces.insert(surface_id)
        {
            return BTreeMap::new();
        }
        bindings.insert(external_id, surface_id);
    }
    bindings
}

pub(super) fn profile_segment_ids(
    definition_id: u32,
    segments: &[crate::feature::FeatureSegment],
    profiles: &[Vec<SketchEntityUse>],
) -> BTreeSet<u32> {
    segments
        .iter()
        .filter(|segment| {
            let entity_id = SketchEntityId(format!(
                "creo:featdefs:sketch_entity#{definition_id}:{}",
                segment.external_id
            ));
            profiles
                .iter()
                .flatten()
                .any(|entity_use| entity_use.entity == entity_id)
        })
        .map(|segment| segment.external_id)
        .collect()
}

pub(super) fn transfer_resolved_revolution_surfaces(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for transform in &scan.features.section_transforms {
        if unique_feature_section_transform(
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )
        .is_none()
        {
            continue;
        }
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if feature_recipe(scan, feature_id) != Some(crate::feature::FeatureRecipeKind::Revolve) {
            continue;
        }
        if unique_feature_revolution_extent_kind(&scan.features.revolution_extents, feature_id)
            .is_none()
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        let extent = feature_revolution_extent(scan, feature_id);
        let Some(axis) = revolution_axis_for_transfer(
            scan,
            ir,
            feature_id,
            definition,
            transform,
            extent.as_ref(),
        ) else {
            continue;
        };
        let points = resolved_section_points(definition);
        let mut generating_ids = definition
            .trim_entities
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| trim_segment_id(definition, row))
            .collect::<BTreeSet<_>>();
        let sketch_id = SketchId(format!("creo:model:sketch#{}", definition.id));
        if let Some(sketch) = ir
            .model
            .sketches
            .iter()
            .find(|sketch| sketch.id == sketch_id)
        {
            let segments = complete_section_segment_rows(definition).to_vec();
            generating_ids.extend(profile_segment_ids(
                definition.id,
                &segments,
                &sketch.profiles,
            ));
        }
        let arc_bindings = definition
            .order_table
            .as_ref()
            .map_or_else(BTreeMap::new, |order| {
                ordered_family_surface_bindings_for_feature(
                    &scan.surfaces.rows,
                    feature_id,
                    &scan.features.entity_tables,
                    order,
                    complete_section_segment_rows(definition)
                        .iter()
                        .filter(|segment| {
                            generating_ids.contains(&segment.external_id)
                                && segment.kind == crate::feature::FeatureSegmentKind::Arc
                        })
                        .map(|segment| segment.external_id),
                    crate::surface::SurfaceKind::TorusOrSphere,
                )
            });
        let spline_bindings = definition
            .order_table
            .as_ref()
            .map_or_else(BTreeMap::new, |order| {
                ordered_family_surface_bindings_for_feature(
                    &scan.surfaces.rows,
                    feature_id,
                    &scan.features.entity_tables,
                    order,
                    semantic_saved_section_entities(definition).filter_map(|entity| match entity {
                        crate::feature::FeatureSavedEntity::Spline(spline) => {
                            order.external_id(spline.entity_id?)
                        }
                        _ => None,
                    }),
                    crate::surface::SurfaceKind::Spline,
                )
            });
        for segment in complete_section_segment_rows(definition)
            .iter()
            .filter(|segment| generating_ids.contains(&segment.external_id))
        {
            let Some(geometry) = resolved_section_segment_geometry(definition, &points, segment)
            else {
                continue;
            };
            let Some(surface) = revolved_section_surface(transform, &geometry, axis) else {
                continue;
            };
            let native_surface = match segment.kind {
                crate::feature::FeatureSegmentKind::Line => {
                    definition.order_table.as_ref().and_then(|order| {
                        ordered_analytic_surface_id_for_feature(
                            &scan.surfaces.rows,
                            &scan.features.entity_tables,
                            feature_id,
                            order,
                            segment.external_id,
                            &surface,
                        )
                    })
                }
                crate::feature::FeatureSegmentKind::Arc => {
                    arc_bindings.get(&segment.external_id).copied()
                }
                crate::feature::FeatureSegmentKind::Point => None,
            };
            let surface_id = native_surface.map_or_else(
                || {
                    SurfaceId(format!(
                        "creo:feature:revolution_surface#{feature_id}:segment{}",
                        segment.external_id
                    ))
                },
                |id| SurfaceId(format!("creo:visibgeom:surface#{id}")),
            );
            if ir.model.surfaces.iter().any(|item| item.id == surface_id) {
                continue;
            }
            annotate(
                annotations,
                &surface_id,
                "FeatDefs",
                segment.offset as u64,
                "evaluated_analytic_revolution_surface",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id: surface_id,
                geometry: surface,
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: native_surface.map_or_else(
                        || {
                            format!(
                                "FeatDefs:revolution#{feature_id}:segment{}",
                                segment.external_id
                            )
                        },
                        |id| format!("VisibGeom:{id}"),
                    ),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }
        if let Some(order) = definition.order_table.as_ref() {
            for (internal_id, section_geometry, offset) in
                semantic_saved_section_entities(definition)
                    .filter_map(saved_section_entity_geometry)
            {
                let Some(external_id) = order.external_id(internal_id) else {
                    continue;
                };
                let Some(surface) = revolved_section_surface(transform, &section_geometry, axis)
                else {
                    continue;
                };
                let Some(native_surface) = ordered_analytic_surface_id_for_feature(
                    &scan.surfaces.rows,
                    &scan.features.entity_tables,
                    feature_id,
                    order,
                    external_id,
                    &surface,
                ) else {
                    continue;
                };
                let surface_id = SurfaceId(format!("creo:visibgeom:surface#{native_surface}"));
                if ir.model.surfaces.iter().any(|item| item.id == surface_id) {
                    continue;
                }
                annotate(
                    annotations,
                    &surface_id,
                    "FeatDefs",
                    offset as u64,
                    "evaluated_saved_analytic_revolution_surface",
                    Exactness::Derived,
                );
                ir.model.surfaces.push(Surface {
                    id: surface_id,
                    geometry: surface,
                    source_object: Some(SourceObjectAssociation {
                        format: "creo".to_string(),
                        object_id: format!("VisibGeom:{native_surface}"),
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                });
                transferred += 1;
            }
        }
        for spline in
            semantic_saved_section_entities(definition).filter_map(|entity| match entity {
                crate::feature::FeatureSavedEntity::Spline(spline) => Some(spline),
                _ => None,
            })
        {
            let suffix = spline.entity_id.map_or_else(
                || format!("offset{}", spline.offset),
                |entity_id| entity_id.to_string(),
            );
            let curve_id = CurveId(format!(
                "creo:featdefs:saved_spline_curve#{}:{suffix}",
                definition.id
            ));
            let Some(CurveGeometry::Nurbs(directrix)) = ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == curve_id)
                .map(|curve| &curve.geometry)
            else {
                continue;
            };
            let Some(surface) = revolved_nurbs_surface(directrix, axis) else {
                continue;
            };
            let native_surface = definition
                .order_table
                .as_ref()
                .and_then(|order| order.external_id(spline.entity_id?))
                .and_then(|external_id| spline_bindings.get(&external_id).copied());
            let Some(native_surface) = native_surface else {
                continue;
            };
            let surface_id = SurfaceId(format!("creo:visibgeom:surface#{native_surface}"));
            let procedural_id = ProceduralSurfaceId(format!(
                "creo:feature:revolution_construction#{feature_id}:{suffix}"
            ));
            if ir.model.surfaces.iter().any(|item| item.id == surface_id) {
                continue;
            }
            annotate(
                annotations,
                &surface_id,
                "FeatDefs",
                spline.offset as u64,
                "evaluated_revolution_surface",
                Exactness::Derived,
            );
            annotate(
                annotations,
                &procedural_id,
                "FeatDefs",
                spline.offset as u64,
                "revolution_surface_construction",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Nurbs(surface),
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{native_surface}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            ir.model.procedural_surfaces.push(ProceduralSurface {
                id: procedural_id,
                surface: surface_id,
                definition: ProceduralSurfaceDefinition::Revolution {
                    directrix: curve_id,
                    axis_origin: axis.origin,
                    axis_direction: axis.direction,
                    angular_interval: [0.0, std::f64::consts::TAU],
                    angular_parameter_interval: None,
                    parameter_interval: [
                        *directrix.knots.first().expect("validated spline knots"),
                        *directrix.knots.last().expect("validated spline knots"),
                    ]
                    .into(),
                    transposed: false,
                    revision_form: None,
                },
                cache_fit_tolerance: None,
                record_bounds: None,
            });
            transferred += 1;
        }
    }
    transferred
}

pub(super) fn transfer_resolved_revolution_vertex_orbit_curves(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut pending = Vec::new();
    for transform in &scan.features.section_transforms {
        if unique_feature_section_transform(
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )
        .is_none()
        {
            continue;
        }
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if feature_recipe(scan, feature_id) != Some(crate::feature::FeatureRecipeKind::Revolve) {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        let extent = feature_revolution_extent(scan, feature_id);
        let Some(axis) = revolution_axis_for_transfer(
            scan,
            ir,
            feature_id,
            definition,
            transform,
            extent.as_ref(),
        ) else {
            continue;
        };
        let sketch_id = SketchId(format!("creo:model:sketch#{}", definition.id));
        for (profile_index, vertices) in connected_sketch_profile_vertices(ir, &sketch_id) {
            for (vertex_index, point) in vertices.iter().enumerate() {
                let Some(geometry) = revolved_section_circle(transform, *point, axis) else {
                    continue;
                };
                pending.push((
                    CurveId(format!(
                        "creo:feature:revolution_vertex_orbit#{feature_id}:profile{profile_index}:vertex{vertex_index}"
                    )),
                    geometry,
                    transform.offset,
                    format!(
                        "FeatDefs:revolution#{feature_id}:profile{profile_index}:vertex{vertex_index}"
                    ),
                ));
            }
        }
    }
    let mut transferred = 0;
    for (id, geometry, offset, object_id) in pending {
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "FeatDefs",
            offset as u64,
            "evaluated_revolution_profile_vertex_orbit",
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id,
            geometry,
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id,
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

pub(super) fn transfer_resolved_extrusion_vertex_orbit_curves(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut pending = Vec::new();
    for transform in &scan.features.section_transforms {
        if unique_feature_section_transform(
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )
        .is_none()
        {
            continue;
        }
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if !feature_allows_linear_extrusion(scan, feature_id) {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        let sketch_id = model_sketch_id(scan, definition);
        for (profile_index, vertices) in connected_sketch_profile_vertices(ir, &sketch_id) {
            for (vertex_index, point) in vertices.iter().enumerate() {
                let Some(geometry) = extruded_section_line(transform, *point) else {
                    continue;
                };
                pending.push((
                    CurveId(format!(
                        "creo:feature:extrusion_vertex_orbit#{feature_id}:profile{profile_index}:vertex{vertex_index}"
                    )),
                    geometry,
                    transform.offset,
                    format!(
                        "FeatDefs:extrusion#{feature_id}:profile{profile_index}:vertex{vertex_index}"
                    ),
                ));
            }
        }
    }
    let mut transferred = 0;
    for (id, geometry, offset, object_id) in pending {
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "FeatDefs",
            offset as u64,
            "evaluated_extrusion_profile_vertex_orbit",
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id,
            geometry,
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id,
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

pub(super) fn feature_dimension_parameter_id(sketch: &SketchId, external_id: u32) -> ParameterId {
    ParameterId(format!(
        "creo:featdefs:parameter#{}:{external_id}",
        sketch_identity_scope(sketch),
    ))
}

pub(super) fn feature_dimension_parameter_row_id(
    sketch: &SketchId,
    external_id: u32,
    occurrence: Option<usize>,
) -> ParameterId {
    occurrence.map_or_else(
        || feature_dimension_parameter_id(sketch, external_id),
        |occurrence| {
            ParameterId(format!(
                "creo:featdefs:parameter#{}:{external_id}:{}",
                sketch_identity_scope(sketch),
                occurrence + 1
            ))
        },
    )
}

pub(super) fn resolved_feature_dimension_parameter<'a>(
    sketch: &SketchId,
    table: &'a crate::feature::FeatureDimensionTable,
    ordinal: usize,
) -> Option<(&'a crate::feature::FeatureDimension, ParameterId)> {
    feature_dimension_table_complete(table).then_some(())?;
    let dimension = table.rows.get(ordinal)?;
    (table
        .rows
        .iter()
        .filter(|candidate| candidate.external_id == dimension.external_id)
        .count()
        == 1)
        .then(|| {
            (
                dimension,
                feature_dimension_parameter_id(sketch, dimension.external_id),
            )
        })
}

pub(super) fn feature_dimension_table_complete(
    table: &crate::feature::FeatureDimensionTable,
) -> bool {
    usize::try_from(table.declared_count).ok() == Some(table.rows.len())
}

pub(super) fn feature_dimension_display(dimension_type: u32) -> Option<DimensionDisplay> {
    match dimension_type {
        0x03 => Some(DimensionDisplay::Radius),
        0x04 => Some(DimensionDisplay::Diameter),
        _ => None,
    }
}

pub(super) fn feature_relation_table_complete(
    table: &crate::feature::FeatureRelationTable,
) -> bool {
    feature_relation_table_expected_rows(table) == Some(table.rows.len())
}

pub(super) fn feature_relation_table_expected_rows(
    table: &crate::feature::FeatureRelationTable,
) -> Option<usize> {
    match table.declared_count {
        0 => None,
        1 => Some(0),
        count => usize::try_from(count - 2).ok(),
    }
}

pub(super) fn feature_relation_table_missing_rows(
    table: &crate::feature::FeatureRelationTable,
) -> usize {
    feature_relation_table_expected_rows(table)
        .map_or(0, |expected| expected.saturating_sub(table.rows.len()))
}

pub(super) fn feature_solver_table_complete(
    header: Option<&crate::feature::FeatureSolverTableHeader>,
    row_count: usize,
) -> bool {
    header.map_or(row_count == 0, |header| {
        usize::try_from(header.declared_count).ok() == Some(row_count)
    })
}

pub(super) fn feature_solver_table_missing_rows(
    header: Option<&crate::feature::FeatureSolverTableHeader>,
    row_count: usize,
) -> usize {
    header.map_or(0, |header| {
        usize::try_from(header.declared_count)
            .unwrap_or(usize::MAX)
            .saturating_sub(row_count)
    })
}

pub(super) fn feature_skamp_table_complete(table: &crate::feature::FeatureRelationTable) -> bool {
    feature_solver_table_complete(table.skamp_header.as_ref(), table.skamps.len())
}

pub(super) fn feature_dimension_parameter_layout(
    keys: &[(SketchId, u32)],
) -> Option<Vec<(u32, String, Option<usize>)>> {
    let mut name_counts = BTreeMap::new();
    let mut local_counts = BTreeMap::new();
    for (sketch, external_id) in keys {
        *name_counts
            .entry((sketch.clone(), *external_id))
            .or_insert(0usize) += 1;
    }
    for key in keys {
        *local_counts.entry(key.clone()).or_insert(0usize) += 1;
    }
    let mut next_ordinals = BTreeMap::<SketchId, u32>::new();
    let mut local_occurrences = BTreeMap::new();
    keys.iter()
        .map(|key @ (sketch, external_id)| {
            let ordinal = next_ordinals.entry(sketch.clone()).or_default();
            let assigned = *ordinal;
            *ordinal = ordinal.checked_add(1)?;
            let occurrence = (local_counts[key] > 1).then(|| {
                let occurrence = local_occurrences.entry(key.clone()).or_insert(0usize);
                let assigned = *occurrence;
                *occurrence += 1;
                assigned
            });
            let name = if name_counts[&(sketch.clone(), *external_id)] == 1 {
                format!("d{external_id}")
            } else if let Some(occurrence) = occurrence {
                format!(
                    "d{}_{}_{}",
                    sketch_identity_scope(sketch),
                    external_id,
                    occurrence + 1
                )
            } else {
                format!("d{}_{}", sketch_identity_scope(sketch), external_id)
            };
            Some((assigned, name, occurrence))
        })
        .collect()
}

pub(super) fn transfer_feature_dimensions(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> (usize, BTreeMap<String, ParameterId>) {
    let feature_ids = ir
        .model
        .features
        .iter()
        .map(|feature| feature.id.clone())
        .collect::<BTreeSet<_>>();
    let mut candidates = Vec::new();
    for definition in &scan.features.definitions {
        let sketch = model_sketch_id(scan, definition);
        let owner = section_owner_feature_id(scan, definition.id, &sketch);
        if !feature_ids.contains(&owner) {
            continue;
        }
        let Some(table) = &definition.dimensions else {
            continue;
        };
        for (source_ordinal, dimension) in table.rows.iter().enumerate() {
            candidates.push((sketch.clone(), definition, source_ordinal, dimension));
        }
    }
    candidates.sort_by_key(|(_, definition, source_ordinal, _)| {
        (definition.offset, definition.id, *source_ordinal)
    });
    let keys = candidates
        .iter()
        .map(|(sketch, _, _, dimension)| (sketch.clone(), dimension.external_id))
        .collect::<Vec<_>>();
    let Some(layout) = feature_dimension_parameter_layout(&keys) else {
        return (0, BTreeMap::new());
    };
    let unique_external_ids = keys
        .iter()
        .fold(BTreeMap::new(), |mut counts, (_, external_id)| {
            *counts.entry(*external_id).or_insert(0usize) += 1;
            counts
        });
    let transferred = layout.len();
    let mut relation_parameters = BTreeMap::new();
    for ((sketch, definition, source_ordinal, dimension), (ordinal, name, occurrence)) in
        candidates.into_iter().zip(layout)
    {
        let owner_id = section_owner_feature_id(scan, definition.id, &sketch);
        let id = feature_dimension_parameter_row_id(&sketch, dimension.external_id, occurrence);
        if unique_external_ids[&dimension.external_id] == 1 {
            relation_parameters.insert(format!("d{}", dimension.external_id), id.clone());
        }
        annotate(
            annotations,
            &id.0,
            "FeatDefs",
            dimension.offset as u64,
            "section_dimension",
            Exactness::Derived,
        );
        let mut properties = BTreeMap::from([
            ("definition_id".to_string(), definition.id.to_string()),
            ("source_ordinal".to_string(), source_ordinal.to_string()),
            ("external_id".to_string(), dimension.external_id.to_string()),
            (
                "dimension_type".to_string(),
                dimension.dimension_type.to_string(),
            ),
            (
                "direction_byte".to_string(),
                dimension.direction_byte.to_string(),
            ),
        ]);
        if let Some(auxiliary) = dimension.auxiliary_value {
            properties.insert("auxiliary_value".to_string(), auxiliary.to_string());
        }
        if dimension.value.is_none() {
            properties.insert("value_state".to_string(), "unresolved".to_string());
        }
        if let Some(token) = &dimension.unresolved_value_token {
            let encoding = match token.as_slice() {
                [0x00, _, _] => Some("three_byte_placeholder"),
                [0x01, _, _, _] => Some("four_byte_placeholder"),
                _ => None,
            };
            if let Some(encoding) = encoding {
                properties.insert("value_encoding".to_string(), encoding.to_string());
                let value_token = token.iter().fold(
                    String::with_capacity(token.len() * 2),
                    |mut encoded, byte| {
                        write!(encoded, "{byte:02x}").expect("writing to a string cannot fail");
                        encoded
                    },
                );
                properties.insert("value_token".to_string(), value_token);
            }
        }
        let expression = dimension
            .value
            .map_or_else(String::new, |value| value.to_string());
        let value = dimension.value.map(|value| match dimension.value_unit {
            crate::feature::DimensionUnit::Radians => ParameterValue::Angle(Angle(value)),
            crate::feature::DimensionUnit::Millimeters => ParameterValue::Length(Length(value)),
            crate::feature::DimensionUnit::SchemaDefined => ParameterValue::Real(value),
        });
        ir.model.parameters.push(DesignParameter {
            id: id.clone(),
            owner: Some(owner_id.clone()),
            ordinal,
            name,
            expression,
            display: feature_dimension_display(dimension.dimension_type),
            value,
            dependencies: Vec::new(),
            properties,
            pmi: None,
            native_ref: Some(feature_sketch_record_id_in_scan(scan, definition)),
        });
        if let Some(feature) = ir
            .model
            .features
            .iter_mut()
            .find(|feature| feature.id == owner_id)
        {
            feature
                .source_content
                .push(FeatureSourceContent::Parameter(id));
        }
    }
    (transferred, relation_parameters)
}

pub(super) fn feature_output_bodies(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Vec<BodyId> {
    let affected_geometry = agreed_feature_geometry_ids(
        &scan.features.affected_ids,
        &scan.features.replay_affected_ids,
        feature_id,
    );
    let generated_surfaces = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
        .map(|row| SurfaceId(format!("creo:visibgeom:surface#{}", row.id)))
        .chain(
            scan.features
                .entity_tables
                .iter()
                .filter(|table| table.feature_id == Some(feature_id))
                .flat_map(|table| &table.surface_ids)
                .map(|surface_id| SurfaceId(format!("creo:visibgeom:surface#{surface_id}"))),
        )
        .chain(
            affected_geometry
                .into_iter()
                .flatten()
                .map(|surface_id| SurfaceId(format!("creo:visibgeom:surface#{surface_id}"))),
        );
    let mut outputs = evaluated_sweep_output_bodies(ir, feature_id);
    let edge_outputs = match feature_edge_selection(scan, ir, feature_id) {
        Some(EdgeSelection::Resolved { edges, .. }) => bodies_containing_edges(ir, &edges),
        _ => Vec::new(),
    };
    if edge_outputs.is_empty() {
        for surface in generated_surfaces {
            for face in ir.model.faces.iter().filter(|face| face.surface == surface) {
                let Some(shell) = ir.model.shells.iter().find(|shell| shell.id == face.shell)
                else {
                    continue;
                };
                let Some(region) = ir
                    .model
                    .regions
                    .iter()
                    .find(|region| region.id == shell.region)
                else {
                    continue;
                };
                if !outputs.contains(&region.body) {
                    outputs.push(region.body.clone());
                }
            }
        }
    } else {
        for body in edge_outputs {
            if !outputs.contains(&body) {
                outputs.push(body);
            }
        }
    }
    outputs
}

pub(super) fn bodies_containing_edges(ir: &CadIr, edges: &[EdgeId]) -> Vec<BodyId> {
    let selected = edges.iter().collect::<BTreeSet<_>>();
    let mut shell_ids = ir
        .model
        .coedges
        .iter()
        .filter(|coedge| selected.contains(&coedge.edge))
        .filter_map(|coedge| {
            let lp = ir
                .model
                .loops
                .iter()
                .find(|lp| lp.id == coedge.owner_loop)?;
            ir.model
                .faces
                .iter()
                .find(|face| face.id == lp.face)
                .map(|face| face.shell.clone())
        })
        .collect::<BTreeSet<_>>();
    shell_ids.extend(
        ir.model
            .shells
            .iter()
            .filter(|shell| shell.wire_edges.iter().any(|edge| selected.contains(edge)))
            .map(|shell| shell.id.clone()),
    );
    ir.model
        .shells
        .iter()
        .filter(|shell| shell_ids.contains(&shell.id))
        .filter_map(|shell| {
            let region = ir
                .model
                .regions
                .iter()
                .find(|region| region.id == shell.region)?;
            ir.model
                .bodies
                .iter()
                .any(|body| body.id == region.body)
                .then(|| region.body.clone())
        })
        .fold(Vec::new(), |mut bodies, body| {
            if !bodies.contains(&body) {
                bodies.push(body);
            }
            bodies
        })
}

pub(super) fn evaluated_sweep_output_bodies(ir: &CadIr, feature_id: u32) -> Vec<BodyId> {
    ["extrusion", "revolution"]
        .into_iter()
        .map(|family| BodyId(format!("creo:feature:{family}#{feature_id}:body")))
        .filter(|id| ir.model.bodies.iter().any(|body| body.id == *id))
        .collect()
}

pub(super) fn evaluated_sweep_body_kind(
    ir: &CadIr,
    family: &str,
    feature_id: u32,
) -> Option<BodyKind> {
    let id = BodyId(format!("creo:feature:{family}#{feature_id}:body"));
    ir.model
        .bodies
        .iter()
        .find(|body| body.id == id)
        .map(|body| body.kind)
}

pub(super) fn new_sheet_output_surface_id(
    feature_id: u32,
    tables: &[crate::feature::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
) -> Option<u32> {
    let owned = tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .collect::<Vec<_>>();
    let unique_table = |class_id| {
        let mut matches = owned
            .iter()
            .copied()
            .filter(|table| table.table_class_id == class_id);
        let table = matches.next()?;
        matches.next().is_none().then_some(table)
    };
    let [owner] = unique_table(67)?.entries.as_slice() else {
        return None;
    };
    let [output] = unique_table(100)?.entries.as_slice() else {
        return None;
    };
    let generated = unique_table(29)?;
    (owner.class_id == 200
        && owner.source_entity_id == Some(feature_id)
        && output.entity_id == owner.entity_id
        && generated.surface_ids.contains(&output.class_id)
        && generated
            .entries
            .iter()
            .any(|entry| entry.entity_id == output.class_id && entry.class_id == 200))
    .then_some(())?;
    let mut surfaces = surface_rows
        .iter()
        .filter(|row| row.id == output.class_id && row.feature_id == feature_id);
    let surface = surfaces.next()?;
    surfaces.next().is_none().then_some(surface.id)
}

pub(super) fn sweep_output_kind(
    scan: &ContainerScan,
    ir: &CadIr,
    family: &str,
    feature_id: u32,
) -> Option<BodyKind> {
    evaluated_sweep_body_kind(ir, family, feature_id).or_else(|| {
        feature_is_sheet_extrusion(scan, feature_id).then_some(())?;
        new_sheet_output_surface_id(
            feature_id,
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        )
        .map(|_| BodyKind::Sheet)
        .or_else(|| {
            current_feature_operation(&scan.features.operations, feature_id)
                .filter(|operation| operation.kind == "Surface")
                .map(|_| BodyKind::Sheet)
        })
    })
}

pub(super) fn sweep_solid(output_kind: Option<BodyKind>) -> Option<bool> {
    output_kind.map(|kind| kind == BodyKind::Solid)
}

pub(super) fn feature_field_text(value: &crate::feature::FeatureFieldValue) -> Option<String> {
    match value {
        crate::feature::FeatureFieldValue::Empty => Some("empty".to_string()),
        crate::feature::FeatureFieldValue::CompactInt(value) => Some(value.to_string()),
        crate::feature::FeatureFieldValue::CompactIntArray(values) => Some(
            values
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
        crate::feature::FeatureFieldValue::EntityReference {
            entity_id,
            terminated,
        } => Some(format!(
            "entity:{entity_id}{}",
            if *terminated { ":terminated" } else { "" }
        )),
        crate::feature::FeatureFieldValue::ScalarArray {
            decoded_values: Some(values),
            ..
        } => Some(
            values
                .iter()
                .map(f64::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
        crate::feature::FeatureFieldValue::ScalarArray {
            decoded_values: None,
            ..
        }
        | crate::feature::FeatureFieldValue::Raw(_) => None,
    }
}

pub(super) fn insert_feature_parameter(
    parameters: &mut BTreeMap<String, String>,
    base: &str,
    value: String,
) {
    if let std::collections::btree_map::Entry::Vacant(entry) = parameters.entry(base.to_string()) {
        entry.insert(value);
        return;
    }
    let mut occurrence = 2;
    loop {
        let name = format!("{base}#{occurrence}");
        if let std::collections::btree_map::Entry::Vacant(entry) = parameters.entry(name) {
            entry.insert(value);
            return;
        }
        occurrence += 1;
    }
}

pub(super) fn feature_parameters(
    scan: &ContainerScan,
    feature_id: u32,
) -> BTreeMap<String, String> {
    let mut parameters = BTreeMap::new();
    for field in scan
        .features
        .choice_fields
        .iter()
        .filter(|field| field.feature_id == feature_id)
    {
        let Some(value) = feature_field_text(&field.value) else {
            continue;
        };
        insert_feature_parameter(
            &mut parameters,
            &format!("choice.{}.{}", field.choice_label, field.name),
            value,
        );
    }
    for affected in scan
        .features
        .affected_ids
        .iter()
        .filter(|record| record.feature_id == feature_id)
    {
        let name = match affected.kind {
            crate::feature::AffectedIdKind::Geometry => "affected_geometry_ids",
            crate::feature::AffectedIdKind::Edges => "affected_edge_ids",
            crate::feature::AffectedIdKind::StrongParents => "strong_parent_feature_ids",
            crate::feature::AffectedIdKind::Parents => "parent_feature_ids",
            crate::feature::AffectedIdKind::Contours => "contour_ids",
            crate::feature::AffectedIdKind::Quilts => "affected_quilt_ids",
        };
        insert_feature_parameter(
            &mut parameters,
            name,
            affected
                .ids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    for affected in scan
        .features
        .replay_affected_ids
        .iter()
        .filter(|record| record.feature_id == feature_id)
    {
        insert_feature_parameter(
            &mut parameters,
            "replay_affected_geometry_ids",
            affected
                .geometry_ids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
        insert_feature_parameter(
            &mut parameters,
            "replay_affected_edge_ids",
            affected
                .edge_ids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
        insert_feature_parameter(
            &mut parameters,
            "replay_geometry_extent",
            match affected.geometry_extent {
                crate::feature::ReplayExtentSource::Explicit => "explicit",
                crate::feature::ReplayExtentSource::Inherited => "inherited",
            }
            .to_string(),
        );
        insert_feature_parameter(
            &mut parameters,
            "replay_edge_extent",
            match affected.edge_extent {
                crate::feature::ReplayExtentSource::Explicit => "explicit",
                crate::feature::ReplayExtentSource::Inherited => "inherited",
            }
            .to_string(),
        );
    }
    for affected in scan
        .features
        .surface_merge_replay_affected_ids
        .iter()
        .filter(|record| record.feature_id == feature_id)
    {
        for (name, ids) in [
            (
                "surface_merge_replay_affected_geometry_ids",
                &affected.geometry_ids,
            ),
            ("surface_merge_replay_affected_edge_ids", &affected.edge_ids),
            (
                "surface_merge_replay_affected_quilt_ids",
                &affected.quilt_ids,
            ),
        ] {
            insert_feature_parameter(
                &mut parameters,
                name,
                ids.iter().map(u32::to_string).collect::<Vec<_>>().join(","),
            );
        }
        for (name, extent) in [
            (
                "surface_merge_replay_geometry_extent",
                affected.geometry_extent,
            ),
            ("surface_merge_replay_edge_extent", affected.edge_extent),
            ("surface_merge_replay_quilt_extent", affected.quilt_extent),
        ] {
            insert_feature_parameter(
                &mut parameters,
                name,
                match extent {
                    crate::feature::ReplayExtentSource::Explicit => "explicit",
                    crate::feature::ReplayExtentSource::Inherited => "inherited",
                }
                .to_string(),
            );
        }
    }
    for direction in scan
        .features
        .loop_restore_directions
        .iter()
        .filter(|record| record.feature_id == feature_id)
    {
        let name = match direction.lane {
            crate::feature::LoopRestoreDirectionLane::Primary => "direction",
            crate::feature::LoopRestoreDirectionLane::Secondary => "direction2",
        };
        insert_feature_parameter(
            &mut parameters,
            &format!("loop_restore.{name}"),
            direction.value.to_string(),
        );
    }
    if let Some(extent) =
        unique_feature_revolution_extent_kind(&scan.features.revolution_extents, feature_id)
    {
        parameters.insert(
            "revolution_extent".to_string(),
            match extent {
                crate::feature::FeatureRevolutionExtentKind::FullTurn => "full_turn",
            }
            .to_string(),
        );
    }
    for table in scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
    {
        for entry in &table.entries {
            let Some(source_entity_id) = entry.source_entity_id else {
                continue;
            };
            insert_feature_parameter(
                &mut parameters,
                &format!(
                    "generated_entity.{}.source_section_entity_id",
                    entry.entity_id
                ),
                source_entity_id.to_string(),
            );
            insert_feature_parameter(
                &mut parameters,
                &format!("generated_entity.{}.entry_class", entry.entity_id),
                entry.class_id.to_string(),
            );
        }
    }
    let owned_definitions = scan
        .features
        .definitions
        .iter()
        .filter(|definition| definition.owner_feature_id == Some(feature_id))
        .collect::<Vec<_>>();
    if let [definition] = owned_definitions.as_slice() {
        parameters.insert(
            "sketch_segment_count".to_string(),
            definition
                .segments
                .as_ref()
                .map_or(0, |segments| segments.rows.len())
                .to_string(),
        );
        parameters.insert(
            "dimension_count".to_string(),
            definition
                .dimensions
                .as_ref()
                .map_or(0, |dimensions| dimensions.rows.len())
                .to_string(),
        );
    }
    for transform in scan
        .features
        .section_transforms
        .iter()
        .filter(|transform| transform.feature_id == Some(feature_id))
    {
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        insert_feature_parameter(
            &mut parameters,
            "profile_sketch",
            model_sketch_id(scan, definition).0,
        );
        if feature_recipe(scan, feature_id) == Some(crate::feature::FeatureRecipeKind::Extrude) {
            insert_feature_parameter(
                &mut parameters,
                "sweep_direction",
                transform
                    .normal
                    .iter()
                    .map(f64::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
    }
    parameters
}

pub(super) fn schema_operation_kind(schema_class: u32) -> Option<&'static str> {
    match schema_class {
        911 => Some("Hole"),
        913 => Some("Round"),
        914 => Some("Chamfer"),
        916 => Some("Cut"),
        917 => Some("Protrusion"),
        923 => Some("Datum Plane"),
        926 => Some("Section"),
        927 => Some("Draft"),
        946 => Some("Surface Merge"),
        _ => None,
    }
}

pub(super) fn feature_reference_name<'a>(
    scan: &'a ContainerScan<'_>,
    feature_id: u32,
) -> Option<&'a str> {
    let mut records = scan
        .features
        .reference_names
        .iter()
        .filter(|record| record.feature_id == feature_id);
    let record = records.next()?;
    records
        .all(|candidate| candidate.name_bytes.as_slice() == record.name_bytes.as_slice())
        .then_some(record.name.as_str())
}

pub(super) fn owned_section_feature_id(scan: &ContainerScan, definition_id: u32) -> Option<u32> {
    let definitions = scan
        .features
        .definitions
        .iter()
        .filter(|definition| definition.id == definition_id)
        .collect::<Vec<_>>();
    let [definition] = definitions.as_slice() else {
        return None;
    };
    let rows = scan
        .features
        .rows
        .iter()
        .filter(|row| {
            row.root_schema_class == Some(926)
                && definition.offset >= row.body_offset
                && definition.offset < row.body_offset.saturating_add(row.body.len())
        })
        .collect::<Vec<_>>();
    let [row] = rows.as_slice() else {
        return None;
    };
    Some(row.feature_id)
}

pub(super) fn section_definition_for_history_feature<'a>(
    scan: &'a ContainerScan<'_>,
    feature_id: u32,
) -> Option<&'a crate::feature::FeatureDefinition> {
    let rows = scan
        .features
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id && row.root_schema_class == Some(926))
        .collect::<Vec<_>>();
    let [row] = rows.as_slice() else {
        return None;
    };
    let definitions = scan
        .features
        .definitions
        .iter()
        .filter(|definition| {
            definition.offset >= row.body_offset
                && definition.offset < row.body_offset.saturating_add(row.body.len())
        })
        .collect::<Vec<_>>();
    let [definition] = definitions.as_slice() else {
        return None;
    };
    Some(*definition)
}

pub(super) fn feature_source_properties(
    scan: &ContainerScan,
    feature_id: u32,
) -> BTreeMap<String, String> {
    let mut properties = BTreeMap::new();
    if let Some(recipe) = current_feature_recipe(&scan.features.operations, feature_id) {
        properties.insert("recipe".to_string(), recipe.name().to_string());
    }
    let schema_class = feature_schema_class(scan, feature_id);
    if let Some(schema_class) = schema_class {
        properties.insert(
            "featdefs_schema_class".to_string(),
            schema_class.to_string(),
        );
    }
    let row_schema_classes = feature_row_schema_classes(scan, feature_id);
    if !row_schema_classes.is_empty() {
        properties.insert(
            "featdefs_row_schema_classes".to_string(),
            row_schema_classes
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    if schema_class.is_none() && !row_schema_classes.is_empty() {
        properties.insert("featdefs_schema_state".to_string(), "ambiguous".to_string());
    }
    properties
}

pub(super) fn feature_dependencies(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    prototype_dependencies: &BTreeMap<u32, Vec<u32>>,
) -> Vec<IrFeatureId> {
    native_feature_dependency_ids(
        &scan.features.affected_ids,
        &scan.features.operations,
        &scan.features.entity_tables,
        &scan.features.surface_merge_replay_affected_ids,
        &scan.surfaces.rows,
        feature_id,
        prototype_dependencies
            .get(&feature_id)
            .map_or(&[], Vec::as_slice),
    )
    .into_iter()
    .filter_map(|dependency| {
        let id = IrFeatureId(format!("creo:model:feature#{dependency}"));
        ir.model
            .features
            .iter()
            .any(|feature| feature.id == id)
            .then_some(id)
    })
    .collect()
}

pub(super) fn native_feature_dependency_ids(
    affected_ids: &[crate::feature::FeatureAffectedIds],
    operations: &[crate::feature::FeatureOperation],
    entity_tables: &[crate::feature::FeatureEntityTable],
    surface_merge_replay_affected_ids: &[crate::feature::FeatureSurfaceMergeAffectedIds],
    surface_rows: &[crate::surface::SurfaceRow],
    feature_id: u32,
    prototype_dependencies: &[u32],
) -> Vec<u32> {
    agreed_feature_parent_ids(affected_ids, feature_id)
        .into_iter()
        .chain(current_feature_recipe_parent(operations, feature_id))
        .chain(prototype_dependencies.iter().copied())
        .chain(surface_merge_entity_dependencies(
            affected_ids,
            surface_merge_replay_affected_ids,
            entity_tables,
            feature_id,
        ))
        .chain(feature_entity_dependencies(entity_tables, feature_id))
        .chain(feature_output_surface_dependencies(
            entity_tables,
            surface_rows,
            feature_id,
        ))
        .chain(surface_transition_dependencies(
            feature_id,
            entity_tables,
            surface_rows,
        ))
        .fold(Vec::new(), |mut dependencies, dependency| {
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
            dependencies
        })
}

pub(super) fn feature_output_surface_dependencies(
    tables: &[crate::feature::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
    feature_id: u32,
) -> Vec<u32> {
    let owned_entities = tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id) && table.table_class_id == 67)
        .flat_map(|table| &table.entries)
        .filter(|entry| entry.class_id == 200 && entry.source_entity_id == Some(feature_id))
        .map(|entry| entry.entity_id)
        .collect::<BTreeSet<_>>();
    tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id) && table.table_class_id == 100)
        .flat_map(|table| &table.entries)
        .filter(|entry| owned_entities.contains(&entry.entity_id))
        .filter_map(|entry| {
            let row = crate::surface::unique_surface_row(surface_rows, entry.class_id)?;
            (row.feature_id != feature_id).then_some(row.feature_id)
        })
        .fold(Vec::new(), |mut dependencies, dependency| {
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
            dependencies
        })
}

pub(super) fn feature_entity_dependencies(
    tables: &[crate::feature::FeatureEntityTable],
    feature_id: u32,
) -> Vec<u32> {
    let mut dependencies = Vec::new();
    for (table_index, table) in tables.iter().enumerate() {
        if table.feature_id != Some(feature_id) || table.table_class_id != 100 {
            continue;
        }
        for (entry_index, entry) in table.entries.iter().enumerate() {
            let consumer_position = (table.offset, entry.offset, table_index, entry_index);
            let producers = tables
                .iter()
                .enumerate()
                .flat_map(|(producer_table_index, producer_table)| {
                    let Some(producer_feature_id) = producer_table.feature_id else {
                        return Vec::new();
                    };
                    if producer_feature_id == feature_id {
                        return Vec::new();
                    }
                    producer_table
                        .entries
                        .iter()
                        .enumerate()
                        .filter_map(|(producer_entry_index, producer_entry)| {
                            let producer_position = (
                                producer_table.offset,
                                producer_entry.offset,
                                producer_table_index,
                                producer_entry_index,
                            );
                            (producer_position < consumer_position
                                && producer_entry.class_id == 200
                                && producer_entry.entity_id == entry.entity_id
                                && producer_entry.source_entity_id.is_some())
                            .then_some(producer_feature_id)
                        })
                        .collect::<Vec<_>>()
                })
                .fold(Vec::new(), |mut producers, producer| {
                    if !producers.contains(&producer) {
                        producers.push(producer);
                    }
                    producers
                });
            let [producer] = producers.as_slice() else {
                continue;
            };
            if !dependencies.contains(producer) {
                dependencies.push(*producer);
            }
        }
    }
    dependencies
}

pub(super) fn feature_entity_producers(
    tables: &[crate::feature::FeatureEntityTable],
) -> BTreeMap<u32, BTreeSet<u32>> {
    tables
        .iter()
        .filter_map(|table| table.feature_id.map(|owner| (owner, table)))
        .flat_map(|(owner, table)| {
            table
                .entries
                .iter()
                .filter(|entry| entry.class_id == 200 && entry.source_entity_id.is_some())
                .map(move |entry| (entry.entity_id, owner))
        })
        .fold(
            BTreeMap::<u32, BTreeSet<u32>>::new(),
            |mut owners, (entity, owner)| {
                owners.entry(entity).or_default().insert(owner);
                owners
            },
        )
}

pub(super) fn agreed_surface_merge_replay_quilt_ids(
    records: &[crate::feature::FeatureSurfaceMergeAffectedIds],
    feature_id: u32,
) -> Option<&[u32]> {
    let mut matches = records
        .iter()
        .filter(|record| record.feature_id == feature_id);
    let ids = matches.next()?.quilt_ids.as_slice();
    matches
        .all(|record| record.quilt_ids.as_slice() == ids)
        .then_some(ids)
}

pub(super) fn surface_merge_quilt_ids<'a>(
    affected_ids: &'a [crate::feature::FeatureAffectedIds],
    replay: &'a [crate::feature::FeatureSurfaceMergeAffectedIds],
    feature_id: u32,
) -> Option<&'a [u32]> {
    if let Some(ids) = agreed_feature_affected_ids(
        affected_ids,
        feature_id,
        crate::feature::AffectedIdKind::Quilts,
    ) {
        return (!ids.is_empty()).then_some(ids);
    }
    if has_feature_affected_ids(
        affected_ids,
        feature_id,
        crate::feature::AffectedIdKind::Quilts,
    ) {
        return None;
    }
    agreed_surface_merge_replay_quilt_ids(replay, feature_id).filter(|ids| !ids.is_empty())
}

pub(super) fn surface_merge_entity_dependencies(
    affected_ids: &[crate::feature::FeatureAffectedIds],
    replay: &[crate::feature::FeatureSurfaceMergeAffectedIds],
    tables: &[crate::feature::FeatureEntityTable],
    feature_id: u32,
) -> Vec<u32> {
    let Some(ids) = surface_merge_quilt_ids(affected_ids, replay, feature_id) else {
        return Vec::new();
    };
    let producers = feature_entity_producers(tables);
    ids.iter()
        .filter_map(|entity_id| {
            let owners = producers.get(entity_id)?;
            let mut owners = owners.iter().copied();
            let owner = owners.next()?;
            (owners.next().is_none() && owner != feature_id).then_some(owner)
        })
        .fold(Vec::new(), |mut dependencies, dependency| {
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
            dependencies
        })
}

pub(super) fn agreed_feature_affected_ids(
    records: &[crate::feature::FeatureAffectedIds],
    feature_id: u32,
    kind: crate::feature::AffectedIdKind,
) -> Option<&[u32]> {
    let mut matches = records
        .iter()
        .filter(|record| record.feature_id == feature_id && record.kind == kind);
    let ids = matches.next()?.ids.as_slice();
    matches
        .all(|record| record.ids.as_slice() == ids)
        .then_some(ids)
}

pub(super) fn has_feature_affected_ids(
    records: &[crate::feature::FeatureAffectedIds],
    feature_id: u32,
    kind: crate::feature::AffectedIdKind,
) -> bool {
    records
        .iter()
        .any(|record| record.feature_id == feature_id && record.kind == kind)
}

pub(super) fn agreed_feature_parent_ids(
    records: &[crate::feature::FeatureAffectedIds],
    feature_id: u32,
) -> Vec<u32> {
    let mut emitted_kinds = Vec::new();
    let mut ids = Vec::new();
    for record in records.iter().filter(|record| {
        record.feature_id == feature_id
            && matches!(
                record.kind,
                crate::feature::AffectedIdKind::StrongParents
                    | crate::feature::AffectedIdKind::Parents
            )
    }) {
        if emitted_kinds.contains(&record.kind) {
            continue;
        }
        emitted_kinds.push(record.kind);
        if let Some(agreed) = agreed_feature_affected_ids(records, feature_id, record.kind) {
            ids.extend_from_slice(agreed);
        }
    }
    ids
}

pub(super) fn surface_prototype_feature_dependencies(
    scan: &ContainerScan,
) -> BTreeMap<u32, Vec<u32>> {
    let mut dependencies = BTreeMap::new();
    for (prototype, row, _) in unique_surface_prototype_associations(scan) {
        let mut fields = prototype
            .parameters
            .iter()
            .filter(|field| field.name == "parent_feats");
        let Some(field) = fields.next() else {
            continue;
        };
        if fields.next().is_some() {
            continue;
        }
        let crate::surface::SurfaceNamedValue::CompactIntArray(consumers) = &field.value else {
            continue;
        };
        add_surface_prototype_feature_dependencies(&mut dependencies, row.feature_id, consumers);
    }
    dependencies
}

pub(super) fn add_surface_prototype_feature_dependencies(
    dependencies: &mut BTreeMap<u32, Vec<u32>>,
    producer: u32,
    consumers: &[u32],
) {
    for &consumer in consumers {
        if consumer == 0 || consumer == producer {
            continue;
        }
        let producers = dependencies.entry(consumer).or_default();
        if !producers.contains(&producer) {
            producers.push(producer);
        }
    }
}

pub(super) fn agreed_feature_replay_geometry_ids(
    records: &[crate::feature::FeatureReplayAffectedIds],
    feature_id: u32,
) -> Option<&[u32]> {
    let mut matches = records
        .iter()
        .filter(|record| record.feature_id == feature_id);
    let ids = matches.next()?.geometry_ids.as_slice();
    matches
        .all(|record| record.geometry_ids.as_slice() == ids)
        .then_some(ids)
}

pub(super) fn agreed_feature_replay_edge_ids(
    records: &[crate::feature::FeatureReplayAffectedIds],
    feature_id: u32,
) -> Option<&[u32]> {
    let mut matches = records
        .iter()
        .filter(|record| record.feature_id == feature_id);
    let ids = matches.next()?.edge_ids.as_slice();
    matches
        .all(|record| record.edge_ids.as_slice() == ids)
        .then_some(ids)
}

pub(super) fn reconcile_feature_links(
    scan: &ContainerScan,
    ir: &mut CadIr,
    prototype_dependencies: &BTreeMap<u32, Vec<u32>>,
) {
    let emitted = ir
        .model
        .features
        .iter()
        .map(|feature| feature.id.clone())
        .collect::<BTreeSet<_>>();
    for feature in &mut ir.model.features {
        let Some(feature_id) = feature
            .id
            .as_str()
            .strip_prefix("creo:model:feature#")
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        let native_dependencies = native_feature_dependency_ids(
            &scan.features.affected_ids,
            &scan.features.operations,
            &scan.features.entity_tables,
            &scan.features.surface_merge_replay_affected_ids,
            &scan.surfaces.rows,
            feature_id,
            prototype_dependencies
                .get(&feature_id)
                .map_or(&[], Vec::as_slice),
        )
        .into_iter()
        .map(|dependency| IrFeatureId(format!("creo:model:feature#{dependency}")))
        .filter(|dependency| emitted.contains(dependency))
        .filter(|dependency| *dependency != feature.id);
        let generated_dependencies = feature_generated_dependencies(&feature.definition);
        feature.dependencies = reconciled_dependencies(
            &feature.id,
            &feature.dependencies,
            native_dependencies.chain(generated_dependencies),
            &emitted,
        );
        if feature.parent.is_none() {
            feature.parent = current_feature_recipe_parent(&scan.features.operations, feature_id)
                .map(|parent| IrFeatureId(format!("creo:model:feature#{parent}")))
                .filter(|parent| *parent != feature.id && emitted.contains(parent));
        }
    }
    let mut remaining = (0..ir.model.features.len()).collect::<Vec<_>>();
    let mut ordered = Vec::with_capacity(remaining.len());
    let mut preceding = BTreeSet::new();
    while !remaining.is_empty() {
        let Some(position) = remaining.iter().position(|index| {
            let feature = &ir.model.features[*index];
            feature
                .dependencies
                .iter()
                .chain(feature.parent.iter())
                .all(|required| !emitted.contains(required) || preceding.contains(required))
        }) else {
            break;
        };
        let index = remaining.remove(position);
        preceding.insert(ir.model.features[index].id.clone());
        ordered.push(index);
    }
    ordered.extend(remaining);
    for (ordinal, index) in ordered.into_iter().enumerate() {
        ir.model.features[index].ordinal = ordinal as u64;
    }
}

pub(super) fn feature_generated_dependencies(definition: &IrFeatureDefinition) -> Vec<IrFeatureId> {
    let face_selections = match definition {
        IrFeatureDefinition::Hole {
            face: Some(face), ..
        }
        | IrFeatureDefinition::Thicken { faces: face, .. }
        | IrFeatureDefinition::KnitSurface { faces: face, .. } => vec![face],
        _ => Vec::new(),
    };
    let edge_selections = match definition {
        IrFeatureDefinition::Fillet { groups } => {
            groups.iter().map(|group| &group.edges).collect::<Vec<_>>()
        }
        IrFeatureDefinition::Chamfer { groups, .. } => {
            groups.iter().map(|group| &group.edges).collect::<Vec<_>>()
        }
        _ => Vec::new(),
    };
    face_selections
        .into_iter()
        .flat_map(|selection| match selection {
            FaceSelection::Generated { faces, .. } => faces
                .iter()
                .map(|face| face.feature.clone())
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .chain(edge_selections.into_iter().flat_map(|selection| {
            match selection {
                EdgeSelection::Generated { edges, .. } => edges
                    .iter()
                    .map(|edge| edge.feature.clone())
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            }
        }))
        .fold(Vec::new(), |mut dependencies, dependency| {
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
            dependencies
        })
}

pub(super) fn reconciled_dependencies(
    feature_id: &IrFeatureId,
    established: &[IrFeatureId],
    native: impl IntoIterator<Item = IrFeatureId>,
    emitted: &BTreeSet<IrFeatureId>,
) -> Vec<IrFeatureId> {
    established
        .iter()
        .cloned()
        .chain(native)
        .filter(|dependency| emitted.contains(dependency))
        .filter(|dependency| dependency != feature_id)
        .fold(Vec::new(), |mut dependencies, dependency| {
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
            dependencies
        })
}

pub(super) fn resolved_revolution_axis(
    definition: &crate::feature::FeatureDefinition,
    transform: &crate::placement::FeatureSectionTransform,
) -> Option<RevolutionAxis> {
    definition.variables.as_ref()?;
    let segments = definition.segments.as_ref()?;
    segments.is_complete().then_some(())?;
    let points = resolved_section_points(definition);
    let candidates = segments
        .rows
        .iter()
        .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Line)
        .filter_map(|segment| {
            let start = points.get(&segment.point_ids[0])?;
            let end = points.get(&segment.point_ids[1])?;
            if start[0] != 0.0 || end[0] != 0.0 || start == end {
                return None;
            }
            let start = section_point_in_model(transform, *start);
            let end = section_point_in_model(transform, *end);
            let direction = normalized(std::array::from_fn(|axis| end[axis] - start[axis]))?;
            Some(RevolutionAxis {
                origin: Point3::new(start[0], start[1], start[2]),
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            })
        })
        .collect::<Vec<_>>();
    let [axis] = candidates.as_slice() else {
        return None;
    };
    Some(*axis)
}

pub(super) fn full_turn_revolution_carrier_axis(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    extent: Option<&RevolveExtent>,
) -> Option<RevolutionAxis> {
    let Some(RevolveExtent::OneSided {
        termination: Termination::Angle {
            angle: Angle(angle),
        },
    }) = extent
    else {
        return None;
    };
    if (angle.abs() - std::f64::consts::TAU).abs() > 1e-12 {
        return None;
    }

    let rows = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
        .collect::<Vec<_>>();
    (!rows.is_empty()).then_some(())?;
    let mut axes = Vec::new();
    let mut plane_normals = Vec::new();
    let mut sphere_centers = Vec::new();
    for row in rows {
        (crate::surface::unique_surface_row(&scan.surfaces.rows, row.id) == Some(row))
            .then_some(())?;
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
        let surfaces = ir
            .model
            .surfaces
            .iter()
            .filter(|surface| surface.id == id)
            .collect::<Vec<_>>();
        let [surface] = surfaces.as_slice() else {
            return None;
        };
        match surface.geometry {
            SurfaceGeometry::Cylinder { origin, axis, .. }
            | SurfaceGeometry::Cone { origin, axis, .. } => {
                axes.push((origin, axis));
            }
            SurfaceGeometry::Torus { center, axis, .. } => {
                axes.push((center, axis));
            }
            SurfaceGeometry::Plane { normal, .. } => plane_normals.push(normal),
            SurfaceGeometry::Sphere { center, .. } => sphere_centers.push(center),
            _ => return None,
        }
    }
    let [(first_origin, first_direction), rest @ ..] = axes.as_slice() else {
        return None;
    };
    let mut direction = normalized([first_direction.x, first_direction.y, first_direction.z])?;
    if direction
        .iter()
        .find(|component| component.abs() > 1e-12)
        .is_some_and(|component| component.is_sign_negative())
    {
        direction = direction.map(|component| -component);
    }
    let first_origin = [first_origin.x, first_origin.y, first_origin.z];
    let axial = dot(first_origin, direction);
    let origin: [f64; 3] = std::array::from_fn(|axis| first_origin[axis] - axial * direction[axis]);
    let scale = first_origin
        .into_iter()
        .chain(
            rest.iter()
                .flat_map(|(origin, _)| [origin.x, origin.y, origin.z]),
        )
        .chain(
            sphere_centers
                .iter()
                .flat_map(|center| [center.x, center.y, center.z]),
        )
        .map(f64::abs)
        .fold(1.0, f64::max);
    for (candidate_origin, candidate_direction) in rest {
        let candidate_direction = normalized([
            candidate_direction.x,
            candidate_direction.y,
            candidate_direction.z,
        ])?;
        ((dot(direction, candidate_direction).abs() - 1.0).abs() <= 1e-10).then_some(())?;
        let displacement = [
            candidate_origin.x - origin[0],
            candidate_origin.y - origin[1],
            candidate_origin.z - origin[2],
        ];
        let radial = cross(displacement, direction);
        (dot(radial, radial).sqrt() <= 1e-9 * scale).then_some(())?;
    }
    for normal in plane_normals {
        let normal = normalized([normal.x, normal.y, normal.z])?;
        ((dot(direction, normal).abs() - 1.0).abs() <= 1e-10).then_some(())?;
    }
    for center in sphere_centers {
        let displacement = [
            center.x - origin[0],
            center.y - origin[1],
            center.z - origin[2],
        ];
        let radial = cross(displacement, direction);
        (dot(radial, radial).sqrt() <= 1e-9 * scale).then_some(())?;
    }
    Some(RevolutionAxis {
        origin: Point3::new(origin[0], origin[1], origin[2]),
        direction: Vector3::new(direction[0], direction[1], direction[2]),
    })
}

pub(super) fn revolution_axis_for_transfer(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    definition: &crate::feature::FeatureDefinition,
    transform: &crate::placement::FeatureSectionTransform,
    extent: Option<&RevolveExtent>,
) -> Option<RevolutionAxis> {
    resolved_revolution_axis(definition, transform)
        .or_else(|| full_turn_revolution_carrier_axis(scan, ir, feature_id, extent))
}

pub(super) fn section_profile_ref(ir: &CadIr, native_ref: String) -> ProfileRef {
    let sketch_id = SketchId(native_ref.replacen("creo:featdefs:sketch#", "creo:model:sketch#", 1));
    let Some(sketch) = ir
        .model
        .sketches
        .iter()
        .find(|sketch| sketch.id == sketch_id)
    else {
        return ProfileRef::Native(native_ref);
    };
    if sketch.profiles.is_empty() {
        ProfileRef::Native(native_ref)
    } else {
        ProfileRef::Sketch(sketch_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GeometryGeneratorFeature {
    pub(super) feature_id: u32,
    pub(super) offset: usize,
    pub(super) surface_ids: Vec<u32>,
    pub(super) curve_ids: Vec<u32>,
}

pub(super) fn geometry_generator_features(scan: &ContainerScan) -> Vec<GeometryGeneratorFeature> {
    let operation_feature_ids = scan
        .features
        .operations
        .iter()
        .map(|operation| operation.feature_id)
        .collect::<BTreeSet<_>>();
    let row_feature_ids = scan
        .features
        .rows
        .iter()
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let datum_feature_ids = scan
        .planes
        .datums
        .iter()
        .map(|datum| datum.feature_id)
        .collect::<BTreeSet<_>>();
    let mut generators = BTreeMap::<u32, GeometryGeneratorFeature>::new();
    for row in &scan.surfaces.rows {
        if row.feature_id == 0 {
            continue;
        }
        let generator =
            generators
                .entry(row.feature_id)
                .or_insert_with(|| GeometryGeneratorFeature {
                    feature_id: row.feature_id,
                    offset: row.offset,
                    surface_ids: Vec::new(),
                    curve_ids: Vec::new(),
                });
        generator.offset = generator.offset.min(row.offset);
        generator.surface_ids.push(row.id);
    }
    for row in &scan.curves.topology_rows {
        if row.feature_id == 0 {
            continue;
        }
        let generator =
            generators
                .entry(row.feature_id)
                .or_insert_with(|| GeometryGeneratorFeature {
                    feature_id: row.feature_id,
                    offset: row.offset,
                    surface_ids: Vec::new(),
                    curve_ids: Vec::new(),
                });
        generator.offset = generator.offset.min(row.offset);
        generator.curve_ids.push(row.id);
    }
    let mut generators = generators
        .into_values()
        .filter(|generator| {
            !operation_feature_ids.contains(&generator.feature_id)
                && !row_feature_ids.contains(&generator.feature_id)
                && !datum_feature_ids.contains(&generator.feature_id)
        })
        .collect::<Vec<_>>();
    generators.sort_by_key(|generator| generator.offset);
    generators
}

/// Return the feature identities that the model-transfer pass will emit.
///
/// Feature definitions are built while the transfer pass is still walking
/// source order. A generated face or edge can therefore name a valid
/// row-backed producer that has not been inserted into `ir.model.features`
/// yet. Derive the complete emitted identity set from the scan instead of
/// using the construction-time prefix of the IR.
pub(super) fn model_feature_ids(scan: &ContainerScan) -> BTreeSet<IrFeatureId> {
    let mut ids = scan
        .features
        .operations
        .iter()
        .map(|operation| operation.feature_id)
        .chain(scan.features.rows.iter().map(|row| row.feature_id))
        .chain(scan.planes.datums.iter().map(|datum| datum.feature_id))
        .map(|feature_id| IrFeatureId(format!("creo:model:feature#{feature_id}")))
        .collect::<BTreeSet<_>>();
    ids.extend(
        geometry_generator_features(scan)
            .into_iter()
            .map(|generator| IrFeatureId(format!("creo:model:feature#{}", generator.feature_id))),
    );
    ids
}

pub(super) fn feature_edge_selection(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<EdgeSelection> {
    let (ids, native) = if let Some(ids) = agreed_feature_affected_ids(
        &scan.features.affected_ids,
        feature_id,
        crate::feature::AffectedIdKind::Edges,
    ) {
        if ids.is_empty() {
            let native = format!("creo:allfeatur:edgs_affected#{feature_id}:");
            return Some(EdgeSelection::Resolved {
                edges: Vec::new(),
                native,
            });
        }
        let native = format!(
            "creo:allfeatur:edgs_affected#{feature_id}:{}",
            ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
        );
        (ids, native)
    } else {
        if has_feature_affected_ids(
            &scan.features.affected_ids,
            feature_id,
            crate::feature::AffectedIdKind::Edges,
        ) {
            return None;
        }
        let ids = agreed_feature_replay_edge_ids(&scan.features.replay_affected_ids, feature_id)?;
        if ids.is_empty() {
            let native = format!("creo:allfeatur:replay_edgs_affected#{feature_id}:");
            return Some(EdgeSelection::Resolved {
                edges: Vec::new(),
                native,
            });
        }
        let native = format!(
            "creo:allfeatur:replay_edgs_affected#{feature_id}:{}",
            ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
        );
        (ids, native)
    };
    let result_edge_ids = feature_result_edge_ids_by_feature(&scan.curves.topology_rows);
    let edges = ids
        .iter()
        .map(|id| EdgeId(format!("creo:visibgeom:edge#{id}")))
        .collect::<Vec<_>>();
    let unique = edges.iter().collect::<BTreeSet<_>>().len() == edges.len();
    if unique
        && edges
            .iter()
            .all(|edge| ir.model.edges.iter().any(|candidate| candidate.id == *edge))
    {
        Some(EdgeSelection::Resolved { edges, native })
    } else if edges
        .iter()
        .any(|edge| ir.model.edges.iter().any(|candidate| candidate.id == *edge))
    {
        // A typed generated selection names one result namespace. A roster
        // that mixes current B-rep edges with absent edges has no neutral
        // mixed identity, so retain the exact native selection.
        Some(EdgeSelection::Native(native))
    } else if let Some(edges) = generated_curve_edge_refs(
        ids,
        &scan.curves.topology_rows,
        &model_feature_ids(scan),
        &result_edge_ids,
    ) {
        Some(EdgeSelection::Generated { edges, native })
    } else {
        Some(EdgeSelection::Native(native))
    }
}

pub(super) fn generated_curve_edge_refs(
    curve_ids: &[u32],
    rows: &[crate::curve::CurveTopologyRow],
    available_features: &BTreeSet<IrFeatureId>,
    result_edge_ids: &BTreeMap<u32, Vec<u32>>,
) -> Option<Vec<GeneratedEdgeRef>> {
    let unique_curve_ids = curve_ids.iter().copied().collect::<BTreeSet<_>>();
    (unique_curve_ids.len() == curve_ids.len()).then_some(())?;
    let unique_rows = crate::topology::uniquely_identified_rows(rows)
        .into_iter()
        .map(|row| (row.id, row))
        .collect::<BTreeMap<_, _>>();
    curve_ids
        .iter()
        .map(|curve_id| {
            let row = unique_rows.get(curve_id)?;
            let feature = IrFeatureId(format!("creo:model:feature#{}", row.feature_id));
            (available_features.contains(&feature)
                && result_edge_ids
                    .get(&row.feature_id)
                    .is_some_and(|ids| ids.contains(curve_id)))
            .then_some(GeneratedEdgeRef {
                feature,
                local_id: format!("curve#{curve_id}"),
            })
        })
        .collect()
}

/// Return the complete feature-local edge roster proven by unique topology rows.
///
/// A decoded `crv_array` topology row is one materialized edge identity. The
/// global curve namespace must contain that identifier exactly once before the
/// row can be exposed in a feature result state.
pub(super) fn feature_result_edge_ids(
    rows: &[crate::curve::CurveTopologyRow],
    feature_id: u32,
) -> Option<Vec<u32>> {
    let mut counts = BTreeMap::<u32, usize>::new();
    for row in rows {
        *counts.entry(row.id).or_default() += 1;
    }
    let feature_rows = rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
        .collect::<Vec<_>>();
    (!feature_rows.is_empty()).then_some(())?;
    feature_rows
        .iter()
        .all(|row| counts.get(&row.id) == Some(&1))
        .then_some(())?;
    Some(feature_rows.into_iter().map(|row| row.id).collect())
}

pub(super) fn feature_result_edge_ids_by_feature(
    rows: &[crate::curve::CurveTopologyRow],
) -> BTreeMap<u32, Vec<u32>> {
    rows.iter()
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|feature_id| {
            feature_result_edge_ids(rows, feature_id).map(|edge_ids| (feature_id, edge_ids))
        })
        .collect()
}

pub(super) fn agreed_feature_geometry_ids<'a>(
    affected_ids: &'a [crate::feature::FeatureAffectedIds],
    replay_affected_ids: &'a [crate::feature::FeatureReplayAffectedIds],
    feature_id: u32,
) -> Option<&'a [u32]> {
    let named = agreed_feature_affected_ids(
        affected_ids,
        feature_id,
        crate::feature::AffectedIdKind::Geometry,
    );
    if named.is_some() {
        return named;
    }
    if has_feature_affected_ids(
        affected_ids,
        feature_id,
        crate::feature::AffectedIdKind::Geometry,
    ) {
        return None;
    }
    agreed_feature_replay_geometry_ids(replay_affected_ids, feature_id)
}

pub(super) fn parallel_support_radius(
    planes: impl IntoIterator<Item = ([f64; 3], [f64; 3])>,
) -> Option<f64> {
    let planes = planes.into_iter().collect::<Vec<_>>();
    let mut radii = Vec::new();
    for first in 0..planes.len() {
        for second in first + 1..planes.len() {
            let first_normal = normalized(planes[first].1)?;
            let second_normal = normalized(planes[second].1)?;
            let alignment = first_normal
                .iter()
                .zip(second_normal)
                .map(|(first, second)| first * second)
                .sum::<f64>();
            if alignment.abs() < 1.0 - 1e-9 {
                continue;
            }
            let gap = planes[second]
                .0
                .iter()
                .zip(planes[first].0)
                .zip(first_normal)
                .map(|((second, first), normal)| (second - first) * normal)
                .sum::<f64>()
                .abs();
            let scale = planes[first]
                .0
                .iter()
                .chain(&planes[second].0)
                .map(|value| value.abs())
                .fold(1.0, f64::max);
            if gap > 1e-9 * scale {
                radii.push(0.5 * gap);
            }
        }
    }
    let radius = *radii.first()?;
    let scale = radius.abs().max(1.0);
    radii
        .iter()
        .all(|candidate| (candidate - radius).abs() <= 1e-9 * scale)
        .then_some(radius)
}

pub(super) fn slot_fillet_cylinder(
    cap_planes: [PlaneEquation; 2],
    support_planes: &[PlaneEquation],
) -> Option<CylinderEquation> {
    let axis = normalized(cap_planes[0].normal)?;
    let second_cap_normal = normalized(cap_planes[1].normal)?;
    if (dot(axis, second_cap_normal).abs() - 1.0).abs() > 1e-10 {
        return None;
    }
    let cap_gap = dot(
        axis,
        std::array::from_fn(|index| cap_planes[1].origin[index] - cap_planes[0].origin[index]),
    )
    .abs();
    if cap_gap <= 1e-9 {
        return None;
    }
    let mut midplanes = Vec::<(PlaneEquation, f64)>::new();
    for first in 0..support_planes.len() {
        let first_normal = normalized(support_planes[first].normal)?;
        if dot(first_normal, axis).abs() > 1e-9 {
            return None;
        }
        for second in first + 1..support_planes.len() {
            let second_normal = normalized(support_planes[second].normal)?;
            if (dot(first_normal, second_normal).abs() - 1.0).abs() > 1e-10 {
                continue;
            }
            let gap = dot(
                first_normal,
                std::array::from_fn(|index| {
                    support_planes[second].origin[index] - support_planes[first].origin[index]
                }),
            )
            .abs();
            if gap <= 1e-9 {
                continue;
            }
            midplanes.push((
                PlaneEquation {
                    origin: std::array::from_fn(|index| {
                        0.5 * (support_planes[first].origin[index]
                            + support_planes[second].origin[index])
                    }),
                    normal: first_normal,
                },
                0.5 * gap,
            ));
        }
    }
    let mut candidates = Vec::<CylinderEquation>::new();
    for first in 0..midplanes.len() {
        for second in first + 1..midplanes.len() {
            let radius = midplanes[first].1;
            let scale = radius.max(midplanes[second].1).max(1.0);
            if (midplanes[second].1 - radius).abs() > 1e-9 * scale
                || dot(midplanes[first].0.normal, midplanes[second].0.normal).abs() > 1.0 - 1e-9
            {
                continue;
            }
            let origin = solve_planes(&[cap_planes[0], midplanes[first].0, midplanes[second].0])?;
            let tangent_to_all = support_planes.iter().all(|plane| {
                let Some(normal) = normalized(plane.normal) else {
                    return false;
                };
                let distance = dot(
                    normal,
                    std::array::from_fn(|index| origin[index] - plane.origin[index]),
                )
                .abs();
                (distance - radius).abs() <= 1e-8 * scale
            });
            if tangent_to_all {
                candidates.push(CylinderEquation {
                    origin,
                    axis,
                    ref_direction: midplanes[first].0.normal,
                    radius,
                });
            }
        }
    }
    let first = *candidates.first()?;
    let scale = first.radius.max(1.0);
    candidates
        .iter()
        .all(|candidate| {
            let origin_delta: [f64; 3] =
                std::array::from_fn(|index| candidate.origin[index] - first.origin[index]);
            (candidate.radius - first.radius).abs() <= 1e-9 * scale
                && (dot(candidate.axis, first.axis).abs() - 1.0).abs() <= 1e-10
                && dot(
                    cross(origin_delta, first.axis),
                    cross(origin_delta, first.axis),
                )
                .sqrt()
                    <= 1e-8 * scale
        })
        .then_some(first)
}

pub(super) fn outline_has_unique_radius_delta(
    frame: crate::surface::TorusOutlineFrame,
    radius: f64,
) -> bool {
    let scale = frame
        .values
        .iter()
        .map(|value| value.abs())
        .fold(radius.abs().max(1.0), f64::max);
    frame.values[..3]
        .iter()
        .zip(&frame.values[3..])
        .filter(|(first, second)| ((*second - *first).abs() - radius).abs() <= 1e-9 * scale)
        .count()
        == 1
}

pub(super) fn coordinate_pair_proves_torus_radii(
    first: [f64; 2],
    second: [f64; 2],
    major_radius: f64,
    minor_radius: f64,
) -> bool {
    let scale = first.iter().chain(&second).map(|value| value.abs()).fold(
        major_radius.abs().max(minor_radius.abs()).max(1.0),
        f64::max,
    );
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    let proves = |outer: f64, minor: f64| {
        close(outer.abs(), 2.0 * (major_radius + minor_radius)) && close(minor.abs(), minor_radius)
    };
    let direct = proves(second[0] - first[0], second[1] - first[1]);
    let swapped = proves(second[1] - first[0], second[0] - first[1]);
    direct ^ swapped
}

pub(super) fn five_coordinate_envelope_proves_torus_radii(
    envelope: crate::surface::Type26FiveCoordinateEnvelope,
    major_radius: f64,
    minor_radius: f64,
) -> bool {
    let [a1, a2, b0, b1, b2] = envelope.values;
    let scale = envelope.values.iter().map(|value| value.abs()).fold(
        major_radius.abs().max(minor_radius.abs()).max(1.0),
        f64::max,
    );
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    close(a1, b0)
        && coordinate_pair_proves_torus_radii([a1, a2], [b1, b2], major_radius, minor_radius)
}

pub(super) fn paired_five_coordinate_sphere_center(
    envelopes: [crate::surface::Type26FiveCoordinateEnvelope; 2],
    radius: f64,
) -> Option<[f64; 3]> {
    (radius.is_finite() && radius > 0.0).then_some(())?;
    let scale = envelopes
        .iter()
        .flat_map(|envelope| envelope.values)
        .map(f64::abs)
        .fold(radius.max(1.0), f64::max);
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    let decoded = envelopes.map(|envelope| {
        let [x_min, z0, y_min, radial_max, z1] = envelope.values;
        (close(x_min, y_min)
            && close(radial_max - x_min, 2.0 * radius)
            && close((z1 - z0).abs(), radius))
        .then_some(([x_min, radial_max], [z0, z1]))
    });
    let [Some((first_radial, first_axial)), Some((second_radial, second_axial))] = decoded else {
        return None;
    };
    (close(first_radial[0], second_radial[0]) && close(first_radial[1], second_radial[1]))
        .then_some(())?;
    let shared = [first_axial[0], first_axial[1]]
        .into_iter()
        .filter(|candidate| {
            [second_axial[0], second_axial[1]]
                .into_iter()
                .any(|other| close(*candidate, other))
        })
        .collect::<Vec<_>>();
    let [center_z] = shared.as_slice() else {
        return None;
    };
    let axial_min = first_axial
        .into_iter()
        .chain(second_axial)
        .fold(f64::INFINITY, f64::min);
    let axial_max = first_axial
        .into_iter()
        .chain(second_axial)
        .fold(f64::NEG_INFINITY, f64::max);
    (close(axial_max - axial_min, 2.0 * radius)
        && close(*center_z - axial_min, radius)
        && close(axial_max - *center_z, radius))
    .then_some([
        0.5 * (first_radial[0] + first_radial[1]),
        0.5 * (first_radial[0] + first_radial[1]),
        *center_z,
    ])
}

pub(super) fn unique_surface_parameter_record<'a>(
    scan: &'a ContainerScan,
    row: &crate::surface::SurfaceRow,
) -> Option<&'a crate::surface::SurfaceParameterRecord> {
    exactly_one(
        scan.surfaces
            .parameters
            .iter()
            .filter(|record| record.offset == row.offset),
    )
}

pub(super) fn unique_section_torus_minor_radius(
    scan: &ContainerScan,
    row: &crate::surface::SurfaceRow,
) -> Option<f64> {
    let section = scan.framing.sections.iter().find(|section| {
        row.offset >= section.offset && row.offset < section.offset.saturating_add(section.length)
    })?;
    let prototype = exactly_one(scan.surfaces.prototype_records.iter().filter(|prototype| {
        prototype.family == crate::surface::SurfacePrototypeFamily::Torus
            && prototype.offset >= section.offset
            && prototype.offset < section.offset.saturating_add(section.length)
    }))?;
    prototype_scalar(prototype, "radius2").filter(|radius| radius.is_finite() && *radius > 0.0)
}

pub(super) fn replayed_torus_minor_radius(
    scan: &ContainerScan,
    row: &crate::surface::SurfaceRow,
    record: &crate::surface::SurfaceParameterRecord,
) -> Option<f64> {
    let prototype_minor_radius = unique_section_torus_minor_radius(scan, row)?;
    record.type26_replayed_minor_radius(row.type_byte, prototype_minor_radius)
}

pub(super) fn prototype_round_radius(
    scan: &ContainerScan,
    rows: &[&crate::surface::SurfaceRow],
) -> Option<f64> {
    (scan.framing.layout == crate::container::Layout::Nd).then_some(())?;
    let feature_id = rows.first()?.feature_id;
    let prototype_radii = unique_surface_prototype_associations(scan)
        .into_iter()
        .filter(|(record, row, _)| {
            record.family == crate::surface::SurfacePrototypeFamily::Torus
                && row.feature_id == feature_id
                && rows.iter().any(|candidate| candidate.offset == row.offset)
        })
        .filter_map(|(record, _, _)| {
            Some((
                prototype_scalar(record, "radius1")?,
                prototype_scalar(record, "radius2")?,
            ))
        })
        .collect::<Vec<_>>();
    let &(radius1, radius2) = prototype_radii.first()?;
    let scale = radius1.abs().max(radius2.abs()).max(1.0);
    (radius1.is_finite()
        && radius1 >= 0.0
        && radius2.is_finite()
        && radius2 > 0.0
        && prototype_radii.iter().all(|candidate| {
            (candidate.0 - radius1).abs() <= 1e-9 * scale
                && (candidate.1 - radius2).abs() <= 1e-9 * scale
        }))
    .then_some(())?;
    rows.iter()
        .all(|row| {
            let Some(record) = unique_surface_parameter_record(scan, row) else {
                return false;
            };
            record.torus_radius_overrides(row.type_byte).is_none()
                && (replayed_torus_minor_radius(scan, row, record)
                    .is_some_and(|radius| radius.to_bits() == radius2.to_bits())
                    || record
                        .torus_outline_frame(row.type_byte)
                        .is_some_and(|frame| outline_has_unique_radius_delta(frame, radius2))
                    || record
                        .type26_five_coordinate_envelope(row.type_byte)
                        .is_some_and(|envelope| {
                            five_coordinate_envelope_proves_torus_radii(envelope, radius1, radius2)
                        })
                    || record
                        .type26_split_coordinate_envelope(row.type_byte)
                        .is_some_and(|envelope| {
                            let [a1, a2, b1, b2] = envelope.values;
                            coordinate_pair_proves_torus_radii([a1, a2], [b1, b2], radius1, radius2)
                        }))
        })
        .then_some(radius2)
}

pub(super) fn round_constant_radius(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<f64> {
    if let Some(radius) = round_direct_radii(scan, feature_id)
        .as_deref()
        .and_then(unique_positive_length)
    {
        return Some(radius);
    }
    let generated_rows = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
        .collect::<Vec<_>>();
    if generated_rows.is_empty() {
        return round_support_radius(scan, ir, feature_id);
    }
    // Unequal decoded rolling-radius samples identify a variable-radius
    // round even when another generated row has no radius proof. A support
    // plane fallback must not turn that incomplete, unequal sample set into
    // a false constant radius.
    if differing_positive_lengths(&round_observed_radii(scan, feature_id)) {
        return None;
    }
    let cylinder_rows = generated_rows
        .iter()
        .filter(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
        .copied()
        .collect::<Vec<_>>();
    if cylinder_rows.is_empty() {
        if generated_rows
            .iter()
            .any(|row| row.kind != crate::surface::SurfaceKind::TorusOrSphere)
        {
            return None;
        }
        return prototype_round_radius(scan, &generated_rows);
    }
    if cylinder_rows.len() != generated_rows.len()
        && generated_rows.iter().all(|row| {
            matches!(
                row.kind,
                crate::surface::SurfaceKind::Cylinder | crate::surface::SurfaceKind::TorusOrSphere
            )
        })
    {
        if let Some(radii) = mixed_round_radius_samples(scan, ir, &generated_rows) {
            return unique_positive_length(&radii);
        }
    }
    let cylinder_radii = round_placed_cylinder_radii(scan, ir, feature_id);
    if differing_positive_lengths(&cylinder_radii) {
        // Independent placed cylinder samples remain decisive when an
        // unresolved toroidal sibling prevents the complete mixed-family
        // witness from being assembled.
        return None;
    }
    if cylinder_radii.len() == cylinder_rows.len()
        && cylinder_rows.len()
            == scan
                .surfaces
                .rows
                .iter()
                .filter(|row| row.feature_id == feature_id)
                .count()
    {
        return unique_positive_length(&cylinder_radii);
    }
    round_support_radius(scan, ir, feature_id)
}

pub(super) fn mixed_round_radius_samples(
    scan: &ContainerScan,
    ir: &CadIr,
    rows: &[&crate::surface::SurfaceRow],
) -> Option<Vec<f64>> {
    let cylinder_rows = rows
        .iter()
        .copied()
        .filter(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
        .collect::<Vec<_>>();
    let torus_rows = rows
        .iter()
        .copied()
        .filter(|row| row.kind == crate::surface::SurfaceKind::TorusOrSphere)
        .collect::<Vec<_>>();
    (!cylinder_rows.is_empty() && !torus_rows.is_empty()).then_some(())?;

    let cylinder_radii = cylinder_rows
        .iter()
        .map(|row| round_cylinder_radius(scan, ir, row))
        .collect::<Option<Vec<_>>>()?;
    let torus_radii = mixed_torus_radius_samples(scan, &torus_rows)?;
    Some(cylinder_radii.into_iter().chain(torus_radii).collect())
}

pub(super) fn mixed_torus_radius_samples(
    scan: &ContainerScan,
    rows: &[&crate::surface::SurfaceRow],
) -> Option<Vec<f64>> {
    let parameters = rows
        .iter()
        .map(|row| Some((row.type_byte, unique_surface_parameter_record(scan, row)?)))
        .collect::<Option<Vec<_>>>()?;
    if parameters
        .iter()
        .all(|(type_byte, record)| record.torus_radius_overrides(*type_byte).is_some())
    {
        return Some(
            parameters
                .iter()
                .filter_map(|(type_byte, record)| record.torus_radius_overrides(*type_byte))
                .map(|overrides| overrides.radius2)
                .collect(),
        );
    }
    if parameters
        .iter()
        .any(|(type_byte, record)| record.torus_radius_overrides(*type_byte).is_some())
    {
        return None;
    }
    prototype_round_radius(scan, rows)
        .and_then(|radius| alloc_filled(rows.len(), radius, "creo_torus_radius_samples").ok())
}

pub(super) fn round_cylinder_radius(
    scan: &ContainerScan,
    ir: &CadIr,
    row: &crate::surface::SurfaceRow,
) -> Option<f64> {
    unique_surface_parameter_record(scan, row)
        .and_then(|record| record.type24_round_radius(row.type_byte))
        .or_else(|| round_placed_cylinder_radius(ir, row))
}

pub(super) fn round_support_radius(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<f64> {
    let affected_ids = agreed_feature_geometry_ids(
        &scan.features.affected_ids,
        &scan.features.replay_affected_ids,
        feature_id,
    )?;
    let support_ids = affected_ids.get(2..)?;
    let support_planes = support_ids
        .iter()
        .filter_map(|id| {
            let surface_id = SurfaceId(format!("creo:visibgeom:surface#{id}"));
            let surface = ir
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == surface_id)?;
            match &surface.geometry {
                SurfaceGeometry::Plane { origin, normal, .. } => Some((
                    [origin.x, origin.y, origin.z],
                    [normal.x, normal.y, normal.z],
                )),
                _ => None,
            }
        })
        .collect::<Vec<_>>();
    (support_planes.len() == support_ids.len()).then(|| parallel_support_radius(support_planes))?
}

pub(super) fn round_placed_cylinder_radii(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Vec<f64> {
    scan.surfaces
        .rows
        .iter()
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
        })
        .filter_map(|row| round_placed_cylinder_radius(ir, row))
        .collect()
}

pub(super) fn round_placed_cylinder_radius(
    ir: &CadIr,
    row: &crate::surface::SurfaceRow,
) -> Option<f64> {
    let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
    ir.model
        .surfaces
        .iter()
        .find(|surface| surface.id == id)
        .and_then(|surface| match surface.geometry {
            SurfaceGeometry::Cylinder { radius, .. } => Some(radius),
            _ => None,
        })
}

pub(super) fn round_direct_radii(scan: &ContainerScan, feature_id: u32) -> Option<Vec<f64>> {
    let generated_rows = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
        .collect::<Vec<_>>();
    (!generated_rows.is_empty()).then_some(())?;
    let radii = round_observed_radii(scan, feature_id);
    (radii.len() == generated_rows.len()).then_some(radii)
}

pub(super) fn round_observed_radii(scan: &ContainerScan, feature_id: u32) -> Vec<f64> {
    scan.surfaces
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
        .filter_map(|row| {
            let parameters = unique_surface_parameter_record(scan, row)?;
            match row.kind {
                crate::surface::SurfaceKind::Cylinder => {
                    parameters.type24_round_radius(row.type_byte)
                }
                crate::surface::SurfaceKind::TorusOrSphere => parameters
                    .torus_radius_overrides(row.type_byte)
                    .map(|overrides| overrides.radius2)
                    .or_else(|| replayed_torus_minor_radius(scan, row, parameters)),
                _ => None,
            }
        })
        .collect()
}

pub(super) fn differing_positive_lengths(values: &[f64]) -> bool {
    let Some(&first) = values.first() else {
        return false;
    };
    if values
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0)
    {
        return false;
    }
    let scale = values
        .iter()
        .copied()
        .map(f64::abs)
        .fold(first.abs().max(1.0), f64::max);
    values
        .iter()
        .any(|value| (*value - first).abs() > 1e-9 * scale)
}

pub(super) fn unique_positive_length(values: &[f64]) -> Option<f64> {
    let value = *values.first()?;
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    let scale = values
        .iter()
        .copied()
        .map(f64::abs)
        .fold(value.abs().max(1.0), f64::max);
    values
        .iter()
        .all(|candidate| {
            candidate.is_finite() && *candidate > 0.0 && (*candidate - value).abs() <= 1e-9 * scale
        })
        .then_some(value)
}

pub(super) fn equal_distance_chamfer_setback(
    cones: &[ConeEquation],
    support_planes: &[PlaneEquation],
) -> Option<f64> {
    (!cones.is_empty() && !support_planes.is_empty()).then_some(())?;
    let setbacks = cones
        .iter()
        .map(|cone| {
            let axis = normalized(cone.axis)?;
            (circular_cone(*cone)
                && cone.radius.abs() <= 1e-12
                && (cone.half_angle - std::f64::consts::FRAC_PI_4).abs() <= 1e-10)
                .then_some(())?;
            support_planes
                .iter()
                .filter_map(|plane| {
                    let normal = normalized(plane.normal)?;
                    let denominator = dot(axis, normal);
                    (denominator.abs() >= 1.0 - 1e-10).then_some(())?;
                    let displacement = [
                        plane.origin[0] - cone.origin[0],
                        plane.origin[1] - cone.origin[1],
                        plane.origin[2] - cone.origin[2],
                    ];
                    let setback = dot(displacement, normal) / denominator;
                    (setback.is_finite() && setback > 1e-12).then_some(setback)
                })
                .min_by(f64::total_cmp)
        })
        .collect::<Option<Vec<_>>>()?;
    unique_positive_length(&setbacks)
}

pub(super) fn chamfer_constant_distance(scan: &ContainerScan, feature_id: u32) -> Option<f64> {
    let rows = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
        .collect::<Vec<_>>();
    (!rows.is_empty()
        && rows
            .iter()
            .all(|row| row.kind == crate::surface::SurfaceKind::Cone))
    .then_some(())?;
    let prototype_frames = unique_surface_prototype_associations(scan)
        .into_iter()
        .filter(|(_, row, _)| row.feature_id == feature_id)
        .filter_map(|(prototype, row, _)| {
            Some((row.offset, crate::surface::prototype_cone_frame(prototype)?))
        })
        .collect::<BTreeMap<_, _>>();
    let cones =
        rows.iter()
            .map(|row| {
                let frame = prototype_frames.get(&row.offset).copied().or_else(|| {
                    unique_surface_parameter_record(scan, row)?.positional_cone_frame
                })?;
                Some(ConeEquation {
                    origin: frame.apex,
                    axis: frame.axis,
                    ref_direction: frame.ref_direction,
                    radius: 0.0,
                    ratio: 1.0,
                    half_angle: frame.half_angle,
                })
            })
            .collect::<Option<Vec<_>>>()?;
    let affected_ids = agreed_feature_geometry_ids(
        &scan.features.affected_ids,
        &scan.features.replay_affected_ids,
        feature_id,
    )?;
    let planes = placed_planes(scan);
    let unplaced_affected_plane = affected_ids.iter().any(|id| {
        scan.surfaces
            .rows
            .iter()
            .any(|row| row.id == *id && row.kind == crate::surface::SurfaceKind::Plane)
            && !planes.contains_key(id)
    });
    (!unplaced_affected_plane).then_some(())?;
    let support_plane_ids = affected_ids
        .iter()
        .copied()
        .filter(|id| planes.contains_key(id))
        .collect::<BTreeSet<_>>();
    let support_planes = support_plane_ids
        .into_iter()
        .filter_map(|id| planes.get(&id).copied())
        .collect::<Vec<_>>();
    equal_distance_chamfer_setback(&cones, &support_planes)
}

pub(super) fn filled_surface_feature_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> IrFeatureDefinition {
    let boundary = unique_feature_profile_definition(
        &scan.features.definitions,
        &scan.features.section_transforms,
        feature_id,
    )
    .map(|definition| model_sketch_id(scan, definition))
    .filter(|sketch| {
        ir.model
            .sketches
            .iter()
            .any(|candidate| candidate.id == *sketch)
    })
    .map_or(
        SurfaceBoundary::Edges(EdgeSelection::Unresolved),
        |sketch| SurfaceBoundary::Path(PathRef::Sketch(sketch)),
    );
    IrFeatureDefinition::FilledSurface {
        boundary,
        support_faces: FaceSelection::Faces(Vec::new()),
        continuity: Some(SurfaceContinuity::Contact),
        boundary_continuities: Vec::new(),
        merge_result: Some(false),
    }
}

pub(super) fn class_100_operand_producers(
    feature_id: u32,
    tables: &[crate::feature::FeatureEntityTable],
) -> Option<Vec<(u32, u32)>> {
    let consumer_tables = tables
        .iter()
        .enumerate()
        .filter(|(_, table)| table.feature_id == Some(feature_id) && table.table_class_id == 100)
        .collect::<Vec<_>>();
    let consumers = consumer_tables
        .iter()
        .flat_map(|(table_index, table)| {
            table
                .entries
                .iter()
                .enumerate()
                .map(move |(entry_index, entry)| {
                    (
                        (table.offset, entry.offset, *table_index, entry_index),
                        entry.entity_id,
                    )
                })
        })
        .collect::<Vec<_>>();
    if consumers.is_empty()
        || consumers
            .iter()
            .map(|(_, entity_id)| entity_id)
            .collect::<BTreeSet<_>>()
            .len()
            != consumers.len()
    {
        return None;
    }
    consumers
        .into_iter()
        .map(|(consumer_position, entity_id)| {
            let producers = tables
                .iter()
                .enumerate()
                .flat_map(|(table_index, table)| {
                    let Some(owner) = table.feature_id else {
                        return Vec::new();
                    };
                    if owner == feature_id {
                        return Vec::new();
                    }
                    table
                        .entries
                        .iter()
                        .enumerate()
                        .filter_map(|(entry_index, entry)| {
                            let position = (table.offset, entry.offset, table_index, entry_index);
                            (position < consumer_position
                                && entry.class_id == 200
                                && entry.entity_id == entity_id
                                && entry.source_entity_id.is_some())
                            .then_some(owner)
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            let [producer] = producers.as_slice() else {
                return None;
            };
            Some((entity_id, *producer))
        })
        .collect()
}

pub(super) fn knit_class_100_operand_entity_ids(
    feature_id: u32,
    tables: &[crate::feature::FeatureEntityTable],
) -> Option<Vec<u32>> {
    class_100_operand_producers(feature_id, tables).map(|operands| {
        operands
            .into_iter()
            .map(|(entity_id, _)| entity_id)
            .collect()
    })
}

pub(super) fn knit_operand_entity_ids(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<(Vec<u32>, &'static str)> {
    if let Some(ids) = surface_merge_quilt_ids(
        &scan.features.affected_ids,
        &scan.features.surface_merge_replay_affected_ids,
        feature_id,
    ) {
        let ids = ids.to_vec();
        if ids.iter().collect::<BTreeSet<_>>().len() == ids.len() {
            return Some((ids, "surface_merge_quilts"));
        }
        return None;
    }
    knit_class_100_operand_entity_ids(feature_id, &scan.features.entity_tables)
        .map(|ids| (ids, "surface_merge_entities"))
}

pub(super) fn knit_operand_surface_ids(
    scan: &ContainerScan,
    feature_id: u32,
    quilt_ids: &[u32],
) -> Option<Vec<u32>> {
    let producers = feature_entity_producers(&scan.features.entity_tables);
    let surface_ids = quilt_ids
        .iter()
        .map(|quilt_id| {
            let mut owners = producers.get(quilt_id)?.iter().copied();
            let producer = owners.next()?;
            if owners.next().is_some() || producer == feature_id {
                return None;
            }
            let matching_entries = scan
                .features
                .entity_tables
                .iter()
                .filter(|table| table.feature_id == Some(producer) && table.table_class_id == 100)
                .flat_map(|table| table.entries.iter())
                .filter(|entry| entry.entity_id == *quilt_id)
                .collect::<Vec<_>>();
            let [entry] = matching_entries.as_slice() else {
                return None;
            };
            let surface = crate::surface::unique_surface_row(&scan.surfaces.rows, entry.class_id)?;
            (surface.feature_id == producer).then_some(entry.class_id)
        })
        .collect::<Option<Vec<_>>>()?;
    (surface_ids.iter().collect::<BTreeSet<_>>().len() == surface_ids.len()).then_some(surface_ids)
}

pub(super) fn knit_surface_feature_definition(
    scan: &ContainerScan,
    feature_id: u32,
) -> IrFeatureDefinition {
    let faces = knit_operand_entity_ids(scan, feature_id).map_or(
        FaceSelection::Unresolved,
        |(quilt_ids, namespace)| {
            let native = format!(
                "creo:allfeatur:{namespace}#{feature_id}:{}",
                quilt_ids
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            );
            let available_features = model_feature_ids(scan);
            let result_surface_ids = feature_result_surface_ids_by_feature(
                &scan.features.entity_tables,
                &scan.surfaces.rows,
            );
            let generated =
                knit_operand_surface_ids(scan, feature_id, &quilt_ids).and_then(|surface_ids| {
                    generated_surface_face_refs(
                        &surface_ids,
                        &scan.surfaces.rows,
                        &result_surface_ids,
                        &available_features,
                    )
                });
            match generated {
                Some(faces) => FaceSelection::Generated { faces, native },
                None => FaceSelection::Native(native),
            }
        },
    );
    IrFeatureDefinition::KnitSurface {
        faces,
        merge_entities: Some(true),
        create_solid: Some(false),
        gap_tolerance: None,
    }
}

/// Select the neutral plane carried by a Draft feature's class-209 entity.
///
/// The class is a neutral-plane carrier only when it has one unambiguous
/// feature-owned surface row and that row is a plane. The table class is not
/// part of the rule: Draft records use more than one enclosing table class.
pub(super) fn draft_neutral_plane_selection(
    scan: &ContainerScan,
    feature_id: u32,
) -> FaceSelection {
    let Some((table, entry)) = exactly_one(
        scan.features
            .entity_tables
            .iter()
            .filter(|table| table.feature_id == Some(feature_id))
            .flat_map(|table| {
                table
                    .entries
                    .iter()
                    .filter(|entry| entry.class_id == 209)
                    .map(move |entry| (table, entry))
            }),
    ) else {
        return FaceSelection::Unresolved;
    };
    if !table.surface_ids.contains(&entry.entity_id) {
        return FaceSelection::Unresolved;
    }
    let Some(surface) = crate::surface::unique_surface_row(&scan.surfaces.rows, entry.entity_id)
        .filter(|surface| {
            surface.feature_id == feature_id && surface.kind == crate::surface::SurfaceKind::Plane
        })
    else {
        return FaceSelection::Unresolved;
    };
    FaceSelection::Native(format!("creo:visibgeom:surface#{}", surface.id))
}

pub(super) fn feature_surface_transitions(
    feature_id: u32,
    tables: &[crate::feature::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
) -> Option<Vec<(u32, u32)>> {
    let owned = tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .collect::<Vec<_>>();
    let outputs = owned
        .iter()
        .flat_map(|table| {
            table
                .entries
                .iter()
                .filter(|entry| entry.class_id == 210)
                .map(move |entry| (*table, entry))
        })
        .collect::<Vec<_>>();
    if outputs.is_empty() {
        return None;
    }
    let predecessors = owned
        .iter()
        .flat_map(|table| table.entries.iter())
        .filter(|entry| entry.class_id == 214 && entry.related_entity_id.is_some())
        .count();
    if predecessors != outputs.len() {
        return None;
    }

    let mut output_ids = BTreeSet::new();
    let mut intermediate_ids = BTreeSet::new();
    let mut source_ids = BTreeSet::new();
    let mut transitions = Vec::with_capacity(outputs.len());
    for (output_table, output) in outputs {
        let intermediate_id = output.related_entity_id?;
        if output.related_entity_state != Some(0)
            || !output_table.surface_ids.contains(&output.entity_id)
            || crate::surface::unique_surface_row(surface_rows, output.entity_id)
                .is_none_or(|row| row.feature_id != feature_id)
            || !output_ids.insert(output.entity_id)
            || !intermediate_ids.insert(intermediate_id)
        {
            return None;
        }
        let mut matches = output_table.entries.iter().filter(|predecessor| {
            predecessor.class_id == 214
                && predecessor.entity_id == intermediate_id
                && predecessor.related_entity_state == Some(0)
                && output_table
                    .non_surface_entity_ids
                    .contains(&predecessor.entity_id)
                && crate::surface::unique_surface_row(surface_rows, predecessor.entity_id).is_none()
        });
        let predecessor = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        let source_id = predecessor.related_entity_id?;
        if crate::surface::unique_surface_row(surface_rows, source_id)
            .is_none_or(|row| row.feature_id == feature_id)
            || !source_ids.insert(source_id)
        {
            return None;
        }
        transitions.push((source_id, output.entity_id));
    }
    output_ids.is_disjoint(&source_ids).then_some(transitions)
}

pub(super) fn surface_transition_dependencies(
    feature_id: u32,
    tables: &[crate::feature::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
) -> Vec<u32> {
    feature_surface_transitions(feature_id, tables, surface_rows)
        .into_iter()
        .flatten()
        .filter_map(|(source_id, _)| {
            crate::surface::unique_surface_row(surface_rows, source_id).map(|row| row.feature_id)
        })
        .fold(Vec::new(), |mut dependencies, dependency| {
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
            dependencies
        })
}

pub(super) fn thicken_plane_offset(
    transitions: &[(u32, u32)],
    planes: &BTreeMap<u32, PlaneEquation>,
    rows: &[crate::surface::SurfaceRow],
) -> Option<(f64, ThickenSide)> {
    let mut offsets = Vec::new();
    for &(source_id, output_id) in transitions {
        let (Some(source), Some(output)) = (planes.get(&source_id), planes.get(&output_id)) else {
            continue;
        };
        let source_row = crate::surface::unique_surface_row(rows, source_id)?;
        let output_row = crate::surface::unique_surface_row(rows, output_id)?;
        (source_row.reversed != output_row.reversed).then_some(())?;
        let source_normal = normalized(source.normal)?.map(|component| {
            if source_row.reversed {
                -component
            } else {
                component
            }
        });
        let output_normal = normalized(output.normal)?;
        if dot(source_normal, output_normal).abs() < 1.0 - 1e-9 {
            return None;
        }
        let displacement = std::array::from_fn(|index| output.origin[index] - source.origin[index]);
        offsets.push(dot(displacement, source_normal));
    }
    let magnitude = unique_positive_length(
        &offsets
            .iter()
            .map(|offset| offset.abs())
            .collect::<Vec<_>>(),
    )?;
    let tolerance = 1e-9 * magnitude.max(1.0);
    let side = if offsets
        .iter()
        .all(|offset| (*offset - magnitude).abs() <= tolerance)
    {
        ThickenSide::Forward
    } else if offsets
        .iter()
        .all(|offset| (*offset + magnitude).abs() <= tolerance)
    {
        ThickenSide::Reverse
    } else {
        return None;
    };
    Some((magnitude, side))
}

/// Return the materialized surface identities that one feature can expose as
/// faces in its regenerated result.
///
/// Every materialized surface in an owned generated-entity table is a
/// result-face identity when its surface row is unique and names the same
/// owning feature. Duplicate identifiers or malformed materialized rows
/// invalidate the complete result state for that feature.
pub(super) fn feature_result_surface_ids(
    tables: &[crate::feature::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
    feature_id: u32,
) -> Option<Vec<u32>> {
    let mut surface_ids = Vec::new();
    let mut seen = BTreeSet::new();
    for table in tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
    {
        for &surface_id in &table.surface_ids {
            let row = crate::surface::unique_surface_row(rows, surface_id)?;
            if row.feature_id != feature_id || !seen.insert(surface_id) {
                return None;
            }
            surface_ids.push(surface_id);
        }
    }
    (!surface_ids.is_empty()).then_some(surface_ids)
}

pub(super) fn feature_result_surface_ids_by_feature(
    tables: &[crate::feature::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> BTreeMap<u32, Vec<u32>> {
    tables
        .iter()
        .filter_map(|table| table.feature_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|feature_id| {
            feature_result_surface_ids(tables, rows, feature_id)
                .map(|surface_ids| (feature_id, surface_ids))
        })
        .collect()
}

pub(super) fn feature_result_topology(
    tables: &[crate::feature::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
    curve_rows: &[crate::curve::CurveTopologyRow],
    feature_id: u32,
) -> Option<FeatureResultTopology> {
    let faces = feature_result_surface_ids(tables, surface_rows, feature_id)
        .unwrap_or_default()
        .into_iter()
        .map(|surface_id| format!("surface#{surface_id}"))
        .collect::<Vec<_>>();
    let edges = feature_result_edge_ids(curve_rows, feature_id)
        .unwrap_or_default()
        .into_iter()
        .map(|curve_id| format!("curve#{curve_id}"))
        .collect::<Vec<_>>();
    (!faces.is_empty() || !edges.is_empty()).then_some(())?;
    Some(FeatureResultTopology {
        id: FeatureResultTopologyId(format!("creo:model:feature-result-topology#{feature_id}")),
        output_of: IrFeatureId(format!("creo:model:feature#{feature_id}")),
        bodies: Vec::new(),
        faces,
        edges,
        vertices: Vec::new(),
        native_ref: None,
    })
}

pub(super) fn generated_surface_face_refs(
    source_ids: &[u32],
    rows: &[crate::surface::SurfaceRow],
    result_surface_ids: &BTreeMap<u32, Vec<u32>>,
    available_features: &BTreeSet<IrFeatureId>,
) -> Option<Vec<GeneratedFaceRef>> {
    source_ids
        .iter()
        .map(|surface_id| {
            let row = crate::surface::unique_surface_row(rows, *surface_id)?;
            let feature = IrFeatureId(format!("creo:model:feature#{}", row.feature_id));
            (available_features.contains(&feature)
                && result_surface_ids
                    .get(&row.feature_id)
                    .is_some_and(|ids| ids.contains(surface_id)))
            .then_some(GeneratedFaceRef {
                feature,
                local_id: format!("surface#{surface_id}"),
            })
        })
        .collect()
}

pub(super) fn emit_feature_result_topologies(scan: &ContainerScan, ir: &mut CadIr) -> usize {
    let mut emitted = 0;
    for feature in &ir.model.features {
        let Some(feature_id) = feature
            .id
            .as_str()
            .strip_prefix("creo:model:feature#")
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        let Some(state) = feature_result_topology(
            &scan.features.entity_tables,
            &scan.surfaces.rows,
            &scan.curves.topology_rows,
            feature_id,
        ) else {
            continue;
        };
        ir.model.feature_result_topologies.push(state);
        emitted += 1;
    }
    emitted
}

pub(super) fn thicken_feature_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> IrFeatureDefinition {
    let transitions = feature_surface_transitions(
        feature_id,
        &scan.features.entity_tables,
        &scan.surfaces.rows,
    );
    let faces = transitions
        .as_ref()
        .map_or(FaceSelection::Unresolved, |transitions| {
            let source_ids = transitions
                .iter()
                .map(|(source_id, _)| *source_id)
                .collect::<Vec<_>>();
            let available_features = model_feature_ids(scan);
            let result_surface_ids = feature_result_surface_ids_by_feature(
                &scan.features.entity_tables,
                &scan.surfaces.rows,
            );
            let native = format!(
                "creo:allfeatur:thicken_source_surfaces#{feature_id}:{}",
                source_ids
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            );
            let faces = source_ids
                .iter()
                .map(|surface_id| FaceId(format!("creo:visibgeom:face#{surface_id}")))
                .collect::<Vec<_>>();
            if faces
                .iter()
                .all(|face| ir.model.faces.iter().any(|candidate| candidate.id == *face))
            {
                FaceSelection::Resolved { faces, native }
            } else if let Some(faces) = generated_surface_face_refs(
                &source_ids,
                &scan.surfaces.rows,
                &result_surface_ids,
                &available_features,
            ) {
                FaceSelection::Generated { faces, native }
            } else {
                FaceSelection::Native(native)
            }
        });
    let offset = transitions.as_deref().and_then(|transitions| {
        thicken_plane_offset(transitions, &placed_planes(scan), &scan.surfaces.rows)
    });
    IrFeatureDefinition::Thicken {
        faces,
        thickness: offset.map(|(magnitude, _)| Length(magnitude)),
        side: offset.map(|(_, side)| side),
    }
}

pub(super) fn schema_feature_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    schema_class: u32,
    kind: &str,
) -> IrFeatureDefinition {
    if numbered_feature_name_has_family(kind, "Fill") {
        return filled_surface_feature_definition(scan, ir, feature_id);
    }
    if numbered_feature_name_has_family(kind, "Thicken") {
        return thicken_feature_definition(scan, ir, feature_id);
    }
    if numbered_feature_name_has_family(kind, "Merge") {
        return knit_surface_feature_definition(scan, feature_id);
    }
    if let Some(definition) = reference_named_feature_definition(kind) {
        return definition;
    }
    if schema_class == 926 {
        let sketch =
            section_definition_for_history_feature(scan, feature_id).and_then(|definition| {
                let section = definition.section_3d.as_ref()?;
                unique_feature_section_transform(
                    &scan.features.section_transforms,
                    definition.id,
                    section.offset,
                )?;
                let sketch = model_sketch_id(scan, definition);
                ir.model
                    .sketches
                    .iter()
                    .any(|candidate| candidate.id == sketch)
                    .then_some(sketch)
            });
        return IrFeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::default(),
            sketch,
        };
    }
    if schema_class == 911 {
        let stepped_form = stepped_hole_form(
            feature_id,
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        );
        let stepped_dimensions = (stepped_form == Some(HoleForm::Counterbore))
            .then(|| counterbore_dimensions(scan, ir, feature_id))
            .flatten();
        let stepped_directed = (stepped_form == Some(HoleForm::Counterbore))
            .then(|| counterbore_directed_placement(scan, ir, feature_id))
            .flatten();
        let stepped_axis = (stepped_form == Some(HoleForm::Counterbore)
            && stepped_directed.is_none())
        .then(|| counterbore_axis_placement(scan, ir, feature_id))
        .flatten();
        let drilled_recipe = simple_drilled_hole_recipe(
            feature_id,
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        );
        let drilled_dimensions = drilled_recipe.and_then(|recipe| {
            simple_drilled_hole_dimensions(
                scan,
                simple_drilled_hole_envelope_spans(scan, recipe.table),
                recipe.dimension_family,
            )
        });
        let drilled_placement =
            drilled_recipe
                .zip(drilled_dimensions)
                .and_then(|(recipe, (diameter, _, depth))| {
                    simple_drilled_hole_placement(scan, recipe.table, diameter, depth)
                });
        let placement = feature_outline_planes(scan, feature_id).and_then(hole_placement);
        let compact_cylinder_id = compact_simple_hole_cylinder_id(
            feature_id,
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        );
        let solved = simple_hole_geometry(scan, feature_id)
            .or_else(|| compact_simple_hole_geometry(scan, feature_id));
        let simple_form = solved.is_some() || compact_cylinder_id.is_some();
        let result_surface_ids = feature_result_surface_ids_by_feature(
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        );
        let available_features = model_feature_ids(scan);
        let face_selection = |surface_id| {
            let native = format!("creo:visibgeom:surface#{surface_id}");
            let face = FaceId(format!("creo:visibgeom:face#{surface_id}"));
            if ir.model.faces.iter().any(|candidate| candidate.id == face) {
                FaceSelection::Resolved {
                    faces: vec![face],
                    native,
                }
            } else if crate::surface::unique_surface_row(&scan.surfaces.rows, surface_id)
                .is_some_and(|row| row.feature_id == feature_id)
            {
                FaceSelection::Native(native)
            } else if let Some(faces) = generated_surface_face_refs(
                &[surface_id],
                &scan.surfaces.rows,
                &result_surface_ids,
                &available_features,
            ) {
                FaceSelection::Generated { faces, native }
            } else {
                FaceSelection::Native(native)
            }
        };
        let (face, position, direction, diameter, extent, bottom) = solved.map_or_else(
            || {
                stepped_directed.map_or_else(
                    || {
                        placement.map_or_else(
                            || {
                                drilled_placement.map_or(
                                    (None, None, None, None, None, None),
                                    |(position, direction)| {
                                        (None, Some(position), Some(direction), None, None, None)
                                    },
                                )
                            },
                            |(entry_surface_id, direction, extent)| {
                                (
                                    Some(face_selection(entry_surface_id)),
                                    None,
                                    Some(Vector3::new(direction[0], direction[1], direction[2])),
                                    None,
                                    Some(extent),
                                    None,
                                )
                            },
                        )
                    },
                    |(entry_surface_id, position, direction, extent)| {
                        (
                            entry_surface_id.map(face_selection),
                            Some(position),
                            Some(direction),
                            None,
                            Some(extent),
                            None,
                        )
                    },
                )
            },
            |hole| {
                let SurfaceGeometry::Cylinder { origin, radius, .. } = hole.geometry else {
                    unreachable!("simple hole helper returns a cylinder")
                };
                (
                    hole.entry_surface_id.map(face_selection),
                    Some(origin),
                    Some(Vector3::new(
                        hole.direction[0],
                        hole.direction[1],
                        hole.direction[2],
                    )),
                    Some(Length(2.0 * radius)),
                    Some(hole.extent),
                    Some(HoleBottom::Flat),
                )
            },
        );
        let drilled_dimensions =
            drilled_dimensions.filter(|(drilled_diameter, _, drilled_depth)| {
                !simple_form
                    && stepped_form.is_none()
                    && stepped_dimensions.is_none()
                    && diameter
                        .as_ref()
                        .is_none_or(|diameter| approximately_equal(diameter.0, *drilled_diameter))
                    && extent.as_ref().is_none_or(|extent| {
                        matches!(extent, Termination::Blind { length }
                        if approximately_equal(length.0, *drilled_depth))
                    })
            });
        let drilled_axis = (drilled_placement.is_none())
            .then(|| {
                let (recipe, (diameter, _, _)) = drilled_recipe.zip(drilled_dimensions)?;
                simple_drilled_hole_axis_placement(scan, recipe.table, diameter)
            })
            .flatten();
        return IrFeatureDefinition::Hole {
            profile: None,
            profile_filter: None,
            face,
            position,
            direction,
            placements: stepped_axis.into_iter().chain(drilled_axis).collect(),
            kind: match (
                drilled_dimensions,
                simple_form,
                stepped_form,
                stepped_dimensions,
            ) {
                (Some((_, drill_point_angle, _)), false, None, None) => HoleKind::SimpleDrilled {
                    drill_point_angle: Angle(drill_point_angle),
                },
                (None, true, None, None) => HoleKind::Simple,
                (None, false, Some(HoleForm::Counterbore), Some((_, diameter, depth))) => {
                    HoleKind::Counterbore {
                        diameter: Length(diameter),
                        depth: Length(depth),
                    }
                }
                (_, _, form, dimensions) => HoleKind::Unresolved {
                    form,
                    counterbore_diameter: dimensions.map(|(_, diameter, _)| Length(diameter)),
                    counterbore_depth: dimensions.map(|(_, _, depth)| Length(depth)),
                    countersink_diameter: None,
                    countersink_angle: None,
                },
            },
            exit_kind: None,
            diameter: diameter
                .or_else(|| drilled_dimensions.map(|(diameter, _, _)| Length(diameter)))
                .or_else(|| stepped_dimensions.map(|(diameter, _, _)| Length(diameter))),
            extent: extent.or_else(|| {
                drilled_dimensions.map(|(_, _, depth)| Termination::Blind {
                    length: Length(depth),
                })
            }),
            bottom,
            taper_angle: None,
            specification: None,
            allow_multi_profile_faces: None,
        };
    }
    if schema_class == 913 {
        let mut observed_radii = round_observed_radii(scan, feature_id);
        observed_radii.extend(round_placed_cylinder_radii(scan, ir, feature_id));
        let radius = round_constant_radius(scan, ir, feature_id).map_or_else(
            || RadiusSpec::Unresolved {
                form: differing_positive_lengths(&observed_radii).then_some(RadiusForm::Variable),
            },
            |radius| RadiusSpec::Constant {
                radius: Length(radius),
            },
        );
        return IrFeatureDefinition::Fillet {
            groups: vec![cadmpeg_ir::features::FilletGroup {
                edges: feature_edge_selection(scan, ir, feature_id)
                    .unwrap_or(EdgeSelection::Unresolved),
                radius,
                tangency_weight: None,
            }],
        };
    }
    if schema_class == 914 {
        return IrFeatureDefinition::Chamfer {
            groups: vec![cadmpeg_ir::features::ChamferGroup {
                edges: feature_edge_selection(scan, ir, feature_id)
                    .unwrap_or(EdgeSelection::Unresolved),
                spec: chamfer_constant_distance(scan, feature_id).map_or_else(
                    || ChamferSpec::Unresolved { form: None },
                    |distance| ChamferSpec::Distance {
                        distance: Length(distance),
                    },
                ),
            }],
            flip_direction: false,
        };
    }
    if schema_class == 927 {
        return IrFeatureDefinition::Draft {
            faces: FaceSelection::Unresolved,
            neutral_plane: draft_neutral_plane_selection(scan, feature_id),
            parting_tool: None,
            pull_direction: None,
            pull_plane: None,
            angle: None,
            outward: None,
        };
    }
    if schema_class == 917
        && !feature_section_sweep_semantics_conflict(scan, feature_id)
        && section_sweep_allows_linear_extrusion(schema_class, feature_recipe(scan, feature_id))
    {
        if let Some(sweep) = circular_sweep_geometry(scan, feature_id) {
            let definition =
                unique_owned_feature_definition(&scan.features.definitions, feature_id).filter(
                    |definition| {
                        sweep
                            .section_definition_id
                            .is_none_or(|definition_id| definition_id == definition.id)
                    },
                );
            let profile = definition.map_or_else(
                || ProfileRef::Unresolved(format!("creo:model:feature#{feature_id}")),
                |definition| {
                    section_profile_ref(ir, feature_sketch_record_id_in_scan(scan, definition))
                },
            );
            let output_kind = sweep_output_kind(scan, ir, "extrusion", feature_id);
            return circular_sweep_feature_definition(
                profile,
                &sweep,
                section_sweep_boolean_operation(
                    feature_recipe_effect(scan, feature_id),
                    kind,
                    output_kind.is_some(),
                    preceding_features_establish_body(ir),
                ),
                sweep_solid(output_kind),
            );
        }
    }
    if feature_recipe(scan, feature_id) == Some(crate::feature::FeatureRecipeKind::Revolve) {
        let extent = feature_revolution_extent(scan, feature_id);
        let transforms = scan
            .features
            .section_transforms
            .iter()
            .filter(|transform| transform.feature_id == Some(feature_id))
            .collect::<Vec<_>>();
        let definition = unique_feature_profile_definition(
            &scan.features.definitions,
            &scan.features.section_transforms,
            feature_id,
        );
        let profile = unique_feature_profile_ref(scan, ir, feature_id);
        let transform = match transforms.as_slice() {
            [transform] => Some(*transform),
            _ => None,
        };
        let axis = definition
            .zip(transform)
            .and_then(|(definition, transform)| resolved_revolution_axis(definition, transform))
            .or_else(|| full_turn_revolution_carrier_axis(scan, ir, feature_id, extent.as_ref()));
        let output_kind = sweep_output_kind(scan, ir, "revolution", feature_id);
        return IrFeatureDefinition::Revolve {
            construction: RevolutionConstruction {
                profile,
                axis,
                extent,
                axis_reference: None,
                solid: sweep_solid(output_kind),
                face_maker_class: None,
                fuse_order: None,
                allow_multi_profile_faces: None,
            },
            op: section_sweep_boolean_operation(
                feature_recipe_effect(scan, feature_id),
                kind,
                output_kind.is_some(),
                preceding_features_establish_body(ir),
            ),
        };
    }
    let recipe = feature_recipe(scan, feature_id);
    if (!feature_section_sweep_semantics_conflict(scan, feature_id)
        && section_sweep_allows_linear_extrusion(schema_class, recipe))
        || feature_is_sheet_extrusion(scan, feature_id)
    {
        let transforms = scan
            .features
            .section_transforms
            .iter()
            .filter(|transform| transform.feature_id == Some(feature_id))
            .collect::<Vec<_>>();
        let definition = match transforms.as_slice() {
            [transform] => {
                unique_feature_definition_for_transform(&scan.features.definitions, transform)
            }
            [] => unique_owned_feature_definition(&scan.features.definitions, feature_id),
            _ => None,
        };
        let section = definition.and_then(|definition| definition.section_3d.as_ref());
        let profile = definition.map(|definition| {
            section_profile_ref(ir, feature_sketch_record_id_in_scan(scan, definition))
        });
        let output_kind = sweep_output_kind(scan, ir, "extrusion", feature_id);
        let op = section_sweep_boolean_operation(
            feature_recipe_effect(scan, feature_id),
            kind,
            output_kind.is_some(),
            preceding_features_establish_body(ir),
        );
        let unique_transform = match transforms.as_slice() {
            [] => Some(None),
            [transform] => Some(Some(*transform)),
            _ => None,
        };
        let extent_and_direction =
            if let ([transform], Some(definition)) = (transforms.as_slice(), definition) {
                generated_arc_cylinder_extent(scan, definition, transform).or_else(|| {
                    feature_plane_equations(scan, feature_id).and_then(|planes| {
                        extrusion_extent_and_direction(transform.origin, transform.normal, planes)
                    })
                })
            } else {
                None
            }
            .or_else(|| generated_cap_plane_extent(scan, ir, feature_id))
            .or_else(|| {
                unique_transform.and_then(|transform| {
                    generated_bounded_cylinder_extent(scan, ir, feature_id, transform)
                })
            })
            .or_else(|| {
                unique_transform.and_then(|transform| {
                    generated_nurbs_translation_extent(scan, ir, feature_id, transform)
                })
            })
            .or_else(|| {
                (transforms.is_empty()).then_some(()).and_then(|()| {
                    generated_rectilinear_plane_extent(scan, ir, feature_id, section)
                })
            });
        let construction = extent_and_direction.map(|(extent, direction)| {
            (
                Some(Vector3::new(direction[0], direction[1], direction[2])),
                extent,
            )
        });
        let (direction, extent) = construction.unwrap_or((None, unresolved_extrude_extent()));
        let profile = profile
            .unwrap_or_else(|| ProfileRef::Unresolved(format!("creo:model:feature#{feature_id}")));
        return IrFeatureDefinition::Extrude {
            profile,
            direction: direction.map_or(
                cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
                cadmpeg_ir::features::ExtrudeDirection::Explicit,
            ),
            start: cadmpeg_ir::features::ExtrudeStart::default(),
            extent,
            op,
            direction_source: None,
            solid: sweep_solid(output_kind),
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        };
    }
    if schema_class == 923 {
        if let Some(datum) = unique_feature_datum_plane(&scan.planes.datums, feature_id) {
            return datum_plane_feature_definition(datum);
        }
        if scan
            .planes
            .datums
            .iter()
            .any(|datum| datum.feature_id == feature_id)
        {
            return IrFeatureDefinition::DatumPlaneUnresolved;
        }
        let plane_ids = scan
            .surfaces
            .rows
            .iter()
            .filter(|row| {
                row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
            })
            .map(|row| row.id)
            .collect::<BTreeSet<_>>();
        let plane_ids = plane_ids.into_iter().collect::<Vec<_>>();
        if plane_ids.len() > 1 {
            return IrFeatureDefinition::DatumPlaneUnresolved;
        }
        if let [surface_id] = plane_ids.as_slice() {
            if crate::surface::unique_surface_row(&scan.surfaces.rows, *surface_id).is_none() {
                return IrFeatureDefinition::DatumPlaneUnresolved;
            }
            if let Some(plane) = placed_planes(scan).get(surface_id) {
                let normal = Vector3::new(plane.normal[0], plane.normal[1], plane.normal[2]);
                let u_axis = placed_plane_surfaces(scan).get(surface_id).map_or_else(
                    || cadmpeg_ir::geometry::derive_reference_direction(normal),
                    |(_, u_axis, _)| Vector3::new(u_axis[0], u_axis[1], u_axis[2]),
                );
                return IrFeatureDefinition::DatumPlane {
                    origin: Point3::new(plane.origin[0], plane.origin[1], plane.origin[2]),
                    normal,
                    u_axis,
                };
            }
            let surface_id = SurfaceId(format!("creo:visibgeom:surface#{surface_id}"));
            let planes = ir
                .model
                .surfaces
                .iter()
                .filter(|surface| surface.id == surface_id)
                .filter_map(|surface| match surface.geometry {
                    SurfaceGeometry::Plane {
                        origin,
                        normal,
                        u_axis,
                    } => Some((origin, normal, u_axis)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if let [(origin, normal, u_axis)] = planes.as_slice() {
                return IrFeatureDefinition::DatumPlane {
                    origin: *origin,
                    normal: *normal,
                    u_axis: *u_axis,
                };
            }
            return IrFeatureDefinition::DatumPlaneUnresolved;
        }
        let definitions = scan
            .features
            .definitions
            .iter()
            .filter(|definition| definition.owner_feature_id == Some(feature_id))
            .collect::<Vec<_>>();
        if let [definition] = definitions.as_slice() {
            if let Some(values) = crate::placement::unique_complete_local_system(definition) {
                let raw_normal: [f64; 3] = values[6..9].try_into().expect("three values");
                let raw_u_axis: [f64; 3] = values[0..3].try_into().expect("three values");
                if let (Some(normal), Some(u_axis)) =
                    (normalized(raw_normal), normalized(raw_u_axis))
                {
                    if dot(normal, u_axis).abs() <= 1e-12 {
                        let origin: [f64; 3] = values[9..12].try_into().expect("three values");
                        return IrFeatureDefinition::DatumPlane {
                            origin: Point3::new(origin[0], origin[1], origin[2]),
                            normal: Vector3::new(normal[0], normal[1], normal[2]),
                            u_axis: Vector3::new(u_axis[0], u_axis[1], u_axis[2]),
                        };
                    }
                }
            }
        }
        return IrFeatureDefinition::DatumPlaneUnresolved;
    }
    if schema_class == 946 {
        return knit_surface_feature_definition(scan, feature_id);
    }
    if schema_class == 979 && kind == "PRT_CSYS_DEF" {
        let definitions = scan
            .features
            .definitions
            .iter()
            .filter(|definition| definition.owner_feature_id == Some(feature_id))
            .collect::<Vec<_>>();
        if let [definition] = definitions.as_slice() {
            if let Some(values) = crate::placement::unique_complete_local_system(definition) {
                let x_axis = normalized(values[0..3].try_into().expect("three values"));
                let y_axis = normalized(values[3..6].try_into().expect("three values"));
                let z_axis = normalized(values[6..9].try_into().expect("three values"));
                let origin: [f64; 3] = values[9..12].try_into().expect("three values");
                if let (Some(x_axis), Some(y_axis), Some(z_axis)) = (x_axis, y_axis, z_axis) {
                    let right_handed = dot(cross(x_axis, y_axis), z_axis) >= 1.0 - 1e-12;
                    let orthogonal = dot(x_axis, y_axis).abs() <= 1e-12
                        && dot(x_axis, z_axis).abs() <= 1e-12
                        && dot(y_axis, z_axis).abs() <= 1e-12;
                    if origin.into_iter().all(f64::is_finite) && orthogonal && right_handed {
                        return IrFeatureDefinition::DatumCoordinateSystem {
                            origin: Point3::new(origin[0], origin[1], origin[2]),
                            x_axis: Vector3::new(x_axis[0], x_axis[1], x_axis[2]),
                            y_axis: Vector3::new(y_axis[0], y_axis[1], y_axis[2]),
                            z_axis: Vector3::new(z_axis[0], z_axis[1], z_axis[2]),
                        };
                    }
                }
            }
        }
        return IrFeatureDefinition::DatumCoordinateSystemUnresolved;
    }
    if numbered_feature_name_has_family(kind, "Extrude")
        && !feature_is_sheet_extrusion(scan, feature_id)
    {
        return extrude_feature_definition_with_profile(
            scan,
            ir,
            feature_id,
            BooleanOp::Unresolved,
        );
    }
    if schema_class == 942
        && class_942_boundary_surface_entity_graph(
            feature_id,
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        )
    {
        return IrFeatureDefinition::BoundarySurfaceUnresolved;
    }
    if schema_operation_kind(schema_class).is_none() {
        if let Some(definition) = named_or_referenced_feature_definition(scan, ir, feature_id, kind)
        {
            return definition;
        }
        if let Some(definition) = unbounded_feature_plane_definition(scan, ir, feature_id) {
            return definition;
        }
    }
    IrFeatureDefinition::Native {
        kind: kind.to_string(),
        parameters: feature_parameters(scan, feature_id),
        properties: BTreeMap::new(),
    }
}

pub(super) fn datum_plane_feature_definition(
    datum: &crate::datum::DatumPlane,
) -> IrFeatureDefinition {
    IrFeatureDefinition::DatumPlane {
        origin: Point3::new(
            datum.normal[0] * datum.offset,
            datum.normal[1] * datum.offset,
            datum.normal[2] * datum.offset,
        ),
        normal: Vector3::new(datum.normal[0], datum.normal[1], datum.normal[2]),
        u_axis: cadmpeg_ir::geometry::derive_reference_direction(Vector3::new(
            datum.normal[0],
            datum.normal[1],
            datum.normal[2],
        )),
    }
}

pub(super) fn unbounded_feature_plane_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<IrFeatureDefinition> {
    let rows = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
        })
        .collect::<Vec<_>>();
    let [row] = rows.as_slice() else {
        return None;
    };
    (row.boundary_type == 1
        && row.next_surface == 0
        && crate::surface::unique_surface_row(&scan.surfaces.rows, row.id) == Some(*row))
    .then_some(())?;
    let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
    let surfaces = ir
        .model
        .surfaces
        .iter()
        .filter(|surface| surface.id == id)
        .collect::<Vec<_>>();
    let [surface] = surfaces.as_slice() else {
        return None;
    };
    let SurfaceGeometry::Plane {
        origin,
        normal,
        u_axis,
    } = surface.geometry
    else {
        return None;
    };
    Some(IrFeatureDefinition::DatumPlane {
        origin,
        normal,
        u_axis,
    })
}

pub(super) fn numbered_feature_name_has_family(name: &str, family: &str) -> bool {
    name.strip_prefix(family)
        .and_then(|suffix| suffix.strip_prefix(' '))
        .is_some_and(|ordinal| {
            !ordinal.is_empty() && ordinal.bytes().all(|byte| byte.is_ascii_digit())
        })
}

pub(super) fn section_sweep_allows_linear_extrusion(
    schema_class: u32,
    recipe: Option<crate::feature::FeatureRecipeKind>,
) -> bool {
    recipe == Some(crate::feature::FeatureRecipeKind::Extrude)
        || (matches!(schema_class, 916 | 917)
            && recipe != Some(crate::feature::FeatureRecipeKind::Revolve))
}

pub(super) fn feature_is_sheet_extrusion(scan: &ContainerScan, feature_id: u32) -> bool {
    feature_schema_class(scan, feature_id) == Some(942)
        && feature_reference_name(scan, feature_id)
            .is_some_and(|name| numbered_feature_name_has_family(name, "Extrude"))
}

pub(super) fn feature_allows_linear_extrusion(scan: &ContainerScan, feature_id: u32) -> bool {
    (!feature_section_sweep_semantics_conflict(scan, feature_id)
        && feature_schema_class(scan, feature_id).is_some_and(|schema_class| {
            section_sweep_allows_linear_extrusion(schema_class, feature_recipe(scan, feature_id))
        }))
        || feature_is_sheet_extrusion(scan, feature_id)
}

pub(super) fn feature_allows_additive_linear_extrusion(
    scan: &ContainerScan,
    feature_id: u32,
) -> bool {
    !feature_section_sweep_semantics_conflict(scan, feature_id)
        && feature_schema_class(scan, feature_id) == Some(917)
        && section_sweep_allows_linear_extrusion(917, feature_recipe(scan, feature_id))
        && feature_recipe_effect(scan, feature_id)
            .is_none_or(|effect| effect == crate::feature::FeatureRecipeEffect::Protrude)
}

pub(super) fn preceding_features_establish_body(ir: &CadIr) -> bool {
    ir.model.features.iter().any(|feature| {
        feature.suppressed != Some(true)
            && (!feature.outputs.is_empty()
                || matches!(
                    feature.definition,
                    IrFeatureDefinition::Extrude {
                        op: BooleanOp::NewBody,
                        ..
                    } | IrFeatureDefinition::Revolve {
                        op: BooleanOp::NewBody,
                        ..
                    }
                ))
    })
}

pub(super) fn section_sweep_boolean_operation(
    recipe_effect: Option<crate::feature::FeatureRecipeEffect>,
    kind: &str,
    has_evaluated_body: bool,
    prior_body: bool,
) -> BooleanOp {
    match recipe_effect {
        Some(crate::feature::FeatureRecipeEffect::Protrude) if prior_body => BooleanOp::Join,
        Some(crate::feature::FeatureRecipeEffect::Protrude) => BooleanOp::NewBody,
        Some(crate::feature::FeatureRecipeEffect::Cut) => BooleanOp::Cut,
        None if kind == "Protrusion" && prior_body => BooleanOp::Join,
        None if kind == "Protrusion" => BooleanOp::NewBody,
        None if kind == "Cut" => BooleanOp::Cut,
        None if has_evaluated_body => BooleanOp::NewBody,
        _ => BooleanOp::Unresolved,
    }
}

pub(super) fn class_942_boundary_surface_entity_graph(
    feature_id: u32,
    tables: &[crate::feature::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
) -> bool {
    let mut generated_surfaces = surface_rows
        .iter()
        .filter(|row| row.feature_id == feature_id);
    let Some(surface) = generated_surfaces.next() else {
        return false;
    };
    if generated_surfaces.next().is_some() || surface.kind != crate::surface::SurfaceKind::Extrusion
    {
        return false;
    }
    let owned = tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .collect::<Vec<_>>();
    let unique_table = |class_id| {
        let mut matches = owned
            .iter()
            .copied()
            .filter(|table| table.table_class_id == class_id);
        let table = matches.next()?;
        matches.next().is_none().then_some(table)
    };
    let Some(generated) = unique_table(29) else {
        return false;
    };
    let Some(topology) = unique_table(94) else {
        return false;
    };
    let Some(owner) = unique_table(67) else {
        return false;
    };
    let Some(output) = unique_table(100) else {
        return false;
    };
    let [owner_entry] = owner.entries.as_slice() else {
        return false;
    };
    matches!(
        generated.entries.as_slice(),
        [entry]
            if entry.class_id == 200
                && entry.entity_id == surface.id
                && entry.source_entity_id == Some(0)
                && generated.surface_ids.as_slice() == [surface.id]
    ) && topology
        .entries
        .iter()
        .map(|entry| entry.class_id)
        .eq([221, 222, 220, 220])
        && owner_entry.class_id == 200
        && owner_entry.source_entity_id == Some(feature_id)
        && matches!(
            output.entries.as_slice(),
            [entry]
                if entry.entity_id == owner_entry.entity_id
                    && entry.class_id == surface.id
        )
}

pub(super) fn named_feature_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    kind: &str,
) -> Option<IrFeatureDefinition> {
    if numbered_feature_name_has_family(kind, "Fill") {
        return Some(filled_surface_feature_definition(scan, ir, feature_id));
    }
    if numbered_feature_name_has_family(kind, "Thicken") {
        return Some(thicken_feature_definition(scan, ir, feature_id));
    }
    if numbered_feature_name_has_family(kind, "Merge") {
        return Some(knit_surface_feature_definition(scan, feature_id));
    }
    if let Some(definition) = surface_intersect_feature_definition(scan, feature_id, kind) {
        return Some(definition);
    }
    if let Some(definition) = reference_named_feature_definition(kind) {
        return Some(definition);
    }
    if matches!(kind, "Protrusion" | "Cut") {
        return Some(extrude_feature_definition_with_profile(
            scan,
            ir,
            feature_id,
            section_sweep_boolean_operation(
                feature_recipe_effect(scan, feature_id),
                kind,
                false,
                preceding_features_establish_body(ir),
            ),
        ));
    }
    let tree_node_role = match kind {
        "Annotation Feature" => Some(FeatureTreeNodeRole::Annotations),
        "Cross Section" | "Querschnitt" => Some(FeatureTreeNodeRole::CrossSections),
        "Body" | "Körper"
            if feature_reference_name(scan, feature_id).is_none()
                && feature_schema_class(scan, feature_id).is_none() =>
        {
            Some(FeatureTreeNodeRole::SolidBodies)
        }
        "Surface"
            if feature_reference_name(scan, feature_id).is_none()
                && feature_schema_class(scan, feature_id).is_none() =>
        {
            Some(FeatureTreeNodeRole::SurfaceBodies)
        }
        _ => None,
    };
    if let Some(role) = tree_node_role {
        return Some(IrFeatureDefinition::TreeNode {
            role,
            children: Vec::new(),
            active_child: None,
        });
    }
    if kind == "Mirror" {
        return Some(IrFeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::Unresolved {
                form: Some(PatternForm::Mirror),
            },
        });
    }
    if kind == "Extrude" || numbered_feature_name_has_family(kind, "Extrude") {
        return Some(extrude_feature_definition_with_profile(
            scan,
            ir,
            feature_id,
            BooleanOp::Unresolved,
        ));
    }
    if kind == "Revolve" || numbered_feature_name_has_family(kind, "Revolve") {
        return Some(revolve_feature_definition_with_profile(
            scan,
            ir,
            feature_id,
            BooleanOp::Unresolved,
        ));
    }
    let schema_class = match kind {
        "Datum Plane" | "Bezugsebene" => 923,
        "Hole" => 911,
        "Round" | "Rundung" => 913,
        "Chamfer" => 914,
        "Draft" | "Schräge" => 927,
        _ => return None,
    };
    Some(schema_feature_definition(
        scan,
        ir,
        feature_id,
        schema_class,
        kind,
    ))
}

pub(super) fn named_or_referenced_feature_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    kind: &str,
) -> Option<IrFeatureDefinition> {
    named_feature_definition(scan, ir, feature_id, kind).or_else(|| {
        feature_reference_name(scan, feature_id)
            .filter(|reference_name| *reference_name != kind)
            .and_then(|reference_name| {
                named_feature_definition(scan, ir, feature_id, reference_name)
            })
    })
}

pub(super) fn extrude_feature_definition(
    profile: ProfileRef,
    op: BooleanOp,
    solid: Option<bool>,
) -> IrFeatureDefinition {
    IrFeatureDefinition::Extrude {
        profile,
        direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
        start: cadmpeg_ir::features::ExtrudeStart::default(),
        extent: unresolved_extrude_extent(),
        op,
        direction_source: None,
        solid,
        face_maker: None,
        inner_wire_taper: None,
        length_along_profile_normal: None,
        allow_multi_profile_faces: None,
    }
}

pub(super) fn extrude_feature_definition_with_profile(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    op: BooleanOp,
) -> IrFeatureDefinition {
    let profile = unique_feature_profile_ref(scan, ir, feature_id)
        .unwrap_or_else(|| ProfileRef::Unresolved(format!("creo:model:feature#{feature_id}")));
    let output_kind = sweep_output_kind(scan, ir, "extrusion", feature_id);
    let op = if op == BooleanOp::Unresolved && output_kind == Some(BodyKind::Sheet) {
        BooleanOp::NewBody
    } else {
        op
    };
    extrude_feature_definition(profile, op, sweep_solid(output_kind))
}

pub(super) fn revolve_feature_definition_with_profile(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    op: BooleanOp,
) -> IrFeatureDefinition {
    IrFeatureDefinition::Revolve {
        construction: RevolutionConstruction {
            profile: unique_feature_profile_ref(scan, ir, feature_id),
            axis: None,
            extent: None,
            axis_reference: None,
            solid: None,
            face_maker_class: None,
            fuse_order: None,
            allow_multi_profile_faces: None,
        },
        op,
    }
}

pub(super) fn unresolved_extrude_extent() -> ExtrudeExtent {
    ExtrudeExtent::OneSided {
        side: ExtrudeSide {
            termination: Termination::Unresolved,
            draft: None,
            offset: None,
        },
    }
}

pub(super) fn surface_intersect_feature_definition(
    scan: &ContainerScan,
    feature_id: u32,
    kind: &str,
) -> Option<IrFeatureDefinition> {
    numbered_feature_name_has_family(kind, "Intersect").then_some(())?;
    let mut surface_tables = scan.features.entity_tables.iter().filter(|table| {
        table.feature_id == Some(feature_id)
            && table.table_class_id == 29
            && !table.surface_ids.is_empty()
    });
    surface_tables.next()?;
    surface_tables.next().is_none().then_some(())?;
    Some(IrFeatureDefinition::SectionShape {
        first: BodySelection::Unresolved,
        second: BodySelection::Unresolved,
        approximate: None,
    })
}

pub(super) fn reference_named_feature_definition(kind: &str) -> Option<IrFeatureDefinition> {
    if numbered_feature_name_has_family(kind, "Boundary Blend") {
        return Some(IrFeatureDefinition::BoundarySurfaceUnresolved);
    }
    if numbered_feature_name_has_family(kind, "Thicken") {
        return Some(IrFeatureDefinition::Thicken {
            faces: FaceSelection::Unresolved,
            thickness: None,
            side: None,
        });
    }
    if numbered_feature_name_has_family(kind, "Merge") {
        return Some(IrFeatureDefinition::KnitSurface {
            faces: FaceSelection::Unresolved,
            merge_entities: Some(true),
            create_solid: Some(false),
            gap_tolerance: None,
        });
    }
    None
}

pub(super) fn retain_native_feature_parameters(
    source_properties: &mut BTreeMap<String, String>,
    definition: &IrFeatureDefinition,
    parameters: &BTreeMap<String, String>,
) {
    if matches!(definition, IrFeatureDefinition::Native { .. }) {
        return;
    }
    for (name, value) in parameters {
        source_properties.insert(format!("native_parameter.{name}"), value.clone());
    }
}
