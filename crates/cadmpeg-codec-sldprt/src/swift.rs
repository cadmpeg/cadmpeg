// SPDX-License-Identifier: Apache-2.0
//! Semantic PMI stored in the SWIFT GDT-analysis object graph.
#![warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::cursor::Cursor;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::ids::PmiId;
use cadmpeg_ir::pmi::{
    DatumReference, DimensionKind, GeometricToleranceKind, PmiAnnotation, PmiDefinition,
    PmiQuantity, PmiTarget, PmiValue,
};

use crate::container::ContainerScan;

const ROOT_CLASS: &str = "PrizMetrik.GdtAnalysisSupport.GdtPart";
const ENTITY_TOKEN: &[u8] = b"\x06Entity";
const MAX_DEPTH: usize = 32;

#[derive(Debug, Clone, PartialEq)]
struct Reference {
    id: String,
    class: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct ObjectSection {
    references: Vec<Reference>,
    entities: Vec<Entity>,
}

#[derive(Debug, Clone, PartialEq)]
struct RelatedObject {
    name: String,
    class: String,
    entity: Entity,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct Entity {
    offset: usize,
    class: String,
    strings: BTreeMap<String, String>,
    integers: BTreeMap<String, i32>,
    doubles: BTreeMap<String, f64>,
    features: ObjectSection,
    annotations: ObjectSection,
    related: Vec<RelatedObject>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct RenderedDimension {
    kind: RenderedDimensionKind,
    value: f64,
    decimal_places: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderedDimensionKind {
    Diameter,
    Depth,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ImplicitNominal {
    Exact(f64),
    Rendered {
        kind: RenderedDimensionKind,
        geometry: f64,
    },
    RenderedOrExact {
        kind: RenderedDimensionKind,
        geometry: f64,
        exact: f64,
    },
}

/// Decode the unique GDT-analysis root carried by a SWIFT schema stream.
pub(crate) fn annotations(
    scan: &ContainerScan<'_>,
    annotations: &mut Annotations,
) -> Vec<PmiAnnotation> {
    let Some((stream, root, rendered_dimensions)) = scan_root(scan) else {
        return Vec::new();
    };
    let mut projected = project(&root);
    enrich_implicit_nominals(&root, &rendered_dimensions, &mut projected);
    for (reference, entity) in root
        .annotations
        .references
        .iter()
        .zip(&root.annotations.entities)
    {
        let prefix = pmi_id(&reference.id).0;
        for annotation in projected.iter().filter(|annotation| {
            annotation.id.0 == prefix || annotation.id.0.starts_with(&format!("{prefix}:"))
        }) {
            crate::annotations::note(
                annotations,
                annotation.id.0.clone(),
                stream.clone(),
                entity.offset as u64,
                "swift_gdt_analysis",
                cadmpeg_ir::Exactness::ByteExact,
            );
        }
    }
    projected
}

pub(crate) fn unsupported_annotation_classes(scan: &ContainerScan<'_>) -> BTreeMap<String, usize> {
    let Some((_, root, _)) = scan_root(scan) else {
        return if has_root_marker(scan) {
            BTreeMap::from([("GdtAnalysisGraphUnresolved".into(), 1)])
        } else {
            BTreeMap::new()
        };
    };
    let mut classes = BTreeMap::new();
    if root.annotations.references.len() != root.annotations.entities.len() {
        classes.insert(
            "GdtAnalysisIncompleteAnnotationRoster".into(),
            root.annotations
                .references
                .len()
                .abs_diff(root.annotations.entities.len()),
        );
        return classes;
    }
    for entity in &root.annotations.entities {
        let class = short_class(&entity.class);
        if class != "GdtDatum" && tolerance_kind(class).is_none() && dimension_kind(class).is_none()
        {
            classes
                .entry(class.to_string())
                .and_modify(|count| *count = count.saturating_add(1))
                .or_insert(1);
        }
    }
    classes
}

fn has_root_marker(scan: &ContainerScan<'_>) -> bool {
    scan.sections().any(|section| {
        section
            .name()
            .is_some_and(|name| name.starts_with("SWIFT/") && name.contains("Schema"))
            && section
                .payload()
                .windows(ROOT_CLASS.len())
                .any(|window| window == ROOT_CLASS.as_bytes())
    })
}

fn scan_root(scan: &ContainerScan<'_>) -> Option<(String, Entity, Vec<RenderedDimension>)> {
    let mut roots = scan
        .sections()
        .filter(|section| {
            section
                .name()
                .is_some_and(|name| name.starts_with("SWIFT/") && name.contains("Schema"))
        })
        .filter_map(|section| {
            parse_unique_root(section.payload()).map(|root| {
                (
                    section.display_name(),
                    root,
                    rendered_dimensions(section.payload()),
                )
            })
        });
    let root = roots.next()?;
    roots.next().is_none().then_some(root)
}

fn parse_unique_root(payload: &[u8]) -> Option<Entity> {
    let mut parsed = None;
    for offset in payload
        .windows(ENTITY_TOKEN.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == ENTITY_TOKEN).then_some(offset))
    {
        let mut cursor = Cursor::with_bounds(payload, offset, payload.len())?;
        let Some(entity) = parse_entity(&mut cursor, 0) else {
            continue;
        };
        if entity.class != ROOT_CLASS {
            continue;
        }
        if parsed.replace(entity).is_some() {
            return None;
        }
    }
    parsed
}

fn parse_entity(cursor: &mut Cursor<'_>, depth: usize) -> Option<Entity> {
    let offset = cursor.position();
    if depth >= MAX_DEPTH || pstr(cursor)? != "Entity" {
        return None;
    }
    let class = pstr(cursor)?.to_string();
    let _assembly = pstr(cursor)?;
    let _version = cursor.u32_le()?;
    let mut entity = Entity {
        offset,
        class,
        ..Entity::default()
    };
    let mut seen_strings = false;
    let mut seen_integers = false;
    let mut seen_doubles = false;
    let mut seen_features = false;
    let mut seen_annotations = false;
    let mut seen_related = false;
    loop {
        match pstr(cursor)? {
            "Strings" if !seen_strings => {
                seen_strings = true;
                entity.strings = read_strings(cursor)?;
            }
            "Integers" if !seen_integers => {
                seen_integers = true;
                entity.integers = read_integers(cursor)?;
            }
            "Doubles" if !seen_doubles => {
                seen_doubles = true;
                entity.doubles = read_doubles(cursor)?;
            }
            "Features" if !seen_features => {
                seen_features = true;
                entity.features = read_objects(cursor, "EndFeatures", depth)?;
            }
            "Annotations" if !seen_annotations => {
                seen_annotations = true;
                entity.annotations = read_objects(cursor, "EndAnnotations", depth)?;
            }
            "RelatedObjects" if !seen_related => {
                seen_related = true;
                entity.related = read_related(cursor, depth)?;
            }
            "EndEntity" => return Some(entity),
            _ => return None,
        }
    }
}

fn read_strings(cursor: &mut Cursor<'_>) -> Option<BTreeMap<String, String>> {
    let count = cursor.u32_le()?;
    let pairs = cursor.read_counted(u64::from(count), 2, |cursor| {
        Some((pstr(cursor)?.to_string(), pstr(cursor)?.to_string()))
    })?;
    if pstr(cursor)? != "EndStrings" {
        return None;
    }
    unique_map(pairs)
}

fn read_integers(cursor: &mut Cursor<'_>) -> Option<BTreeMap<String, i32>> {
    let count = cursor.u32_le()?;
    let pairs = cursor.read_counted(u64::from(count), 5, |cursor| {
        Some((pstr(cursor)?.to_string(), cursor.i32_le()?))
    })?;
    if pstr(cursor)? != "EndIntegers" {
        return None;
    }
    unique_map(pairs)
}

fn read_doubles(cursor: &mut Cursor<'_>) -> Option<BTreeMap<String, f64>> {
    let count = cursor.u32_le()?;
    let pairs = cursor.read_counted(u64::from(count), 9, |cursor| {
        Some((pstr(cursor)?.to_string(), cursor.f64_le()?))
    })?;
    if pstr(cursor)? != "EndDoubles" {
        return None;
    }
    unique_map(pairs)
}

fn unique_map<T>(pairs: Vec<(String, T)>) -> Option<BTreeMap<String, T>> {
    let mut values = BTreeMap::new();
    for (name, value) in pairs {
        if values.insert(name, value).is_some() {
            return None;
        }
    }
    Some(values)
}

fn read_objects(cursor: &mut Cursor<'_>, end: &str, depth: usize) -> Option<ObjectSection> {
    let count = cursor.u32_le()?;
    let references = cursor.read_counted(u64::from(count), 2, |cursor| {
        Some(Reference {
            id: pstr(cursor)?.to_string(),
            class: pstr(cursor)?.to_string(),
        })
    })?;
    let mut entities = Vec::new();
    while peek_pstr(cursor)? == "Entity" {
        if entities.len() >= references.len() {
            return None;
        }
        entities.push(parse_entity(cursor, depth.checked_add(1)?)?);
    }
    if !references
        .iter()
        .zip(&entities)
        .all(|(reference, entity)| reference_matches_entity(reference, entity))
    {
        return None;
    }
    if pstr(cursor)? != end {
        return None;
    }
    Some(ObjectSection {
        references,
        entities,
    })
}

fn read_related(cursor: &mut Cursor<'_>, depth: usize) -> Option<Vec<RelatedObject>> {
    let count = cursor.u32_le()?;
    let descriptors = cursor.read_counted(u64::from(count), 2, |cursor| {
        Some((pstr(cursor)?.to_string(), pstr(cursor)?.to_string()))
    })?;
    let mut related = Vec::with_capacity(descriptors.len());
    for (name, class) in descriptors {
        let entity = parse_entity(cursor, depth.checked_add(1)?)?;
        if class
            .split_once(',')
            .map_or(class.as_str(), |(name, _)| name)
            != entity.class
        {
            return None;
        }
        related.push(RelatedObject {
            name,
            class,
            entity,
        });
    }
    if pstr(cursor)? != "EndRelatedObjects" {
        return None;
    }
    Some(related)
}

fn reference_matches_entity(reference: &Reference, entity: &Entity) -> bool {
    reference
        .class
        .split_once(',')
        .map_or(reference.class.as_str(), |(class, _)| class)
        == entity.class
}

fn pstr<'a>(cursor: &mut Cursor<'a>) -> Option<&'a str> {
    let len = usize::from(cursor.u8()?);
    std::str::from_utf8(cursor.take(len)?).ok()
}

fn peek_pstr<'a>(cursor: &Cursor<'a>) -> Option<&'a str> {
    let mut probe = *cursor;
    pstr(&mut probe)
}

fn project(root: &Entity) -> Vec<PmiAnnotation> {
    if root.annotations.references.len() != root.annotations.entities.len() {
        return Vec::new();
    }
    let rows = root
        .annotations
        .references
        .iter()
        .zip(&root.annotations.entities)
        .collect::<Vec<_>>();
    let feature_index = feature_index(root);
    let datum_ids = rows
        .iter()
        .filter(|(_, entity)| {
            short_class(&entity.class) == "GdtDatum"
                && !suppressed(entity)
                && entity
                    .strings
                    .get("DatumIdentifier")
                    .is_some_and(|value| !value.is_empty())
        })
        .map(|(reference, _)| (reference.id.as_str(), pmi_id(&reference.id)))
        .collect::<BTreeMap<_, _>>();
    let mut projected = Vec::new();
    let mut datum_systems = Vec::<(Vec<DatumReference>, PmiId)>::new();
    for (reference, entity) in &rows {
        if suppressed(entity) {
            continue;
        }
        if let Some(annotation) = project_datum(reference, entity, &feature_index) {
            projected.push(annotation);
        }
    }
    for (reference, entity) in rows {
        if suppressed(entity) || short_class(&entity.class) == "GdtDatum" {
            continue;
        }
        if let Some((system, mut annotation)) =
            project_tolerance(reference, entity, &datum_ids, &feature_index)
        {
            if let Some(system) = system {
                let PmiDefinition::DatumSystem { references } = &system.definition else {
                    unreachable!("projected datum system definition");
                };
                if let Some((_, id)) = datum_systems
                    .iter()
                    .find(|(candidate, _)| candidate == references)
                {
                    let PmiDefinition::GeometricTolerance { datum_system, .. } =
                        &mut annotation.definition
                    else {
                        unreachable!("projected geometric tolerance definition");
                    };
                    *datum_system = Some(id.clone());
                } else {
                    datum_systems.push((references.clone(), system.id.clone()));
                    projected.push(system);
                }
            }
            projected.push(annotation);
            if short_class(&entity.class) == "GdtCompositeSurfaceProfile" {
                if let Some(lower_tier) =
                    project_lower_profile_tier(reference, entity, &feature_index)
                {
                    projected.push(lower_tier);
                }
            }
        } else if let Some(annotation) = project_dimension(reference, entity, &feature_index) {
            projected.push(annotation);
        }
    }
    projected
}

fn project_datum(
    reference: &Reference,
    entity: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<PmiAnnotation> {
    let identification = entity
        .strings
        .get("DatumIdentifier")
        .filter(|value| !value.is_empty())?
        .clone();
    (short_class(&entity.class) == "GdtDatum").then(|| PmiAnnotation {
        id: pmi_id(&reference.id),
        name: object_name(entity),
        targets: targets(entity, feature_index),
        definition: PmiDefinition::Datum { identification },
    })
}

fn project_tolerance(
    reference: &Reference,
    entity: &Entity,
    datum_ids: &BTreeMap<&str, PmiId>,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<(Option<PmiAnnotation>, PmiAnnotation)> {
    let kind = tolerance_kind(short_class(&entity.class))?;
    let magnitude = finite_nonnegative(entity.doubles.get("Tolerance").copied()?)?;
    let references = datum_references(entity, datum_ids);
    let system = (!references.is_empty()).then(|| {
        let id = PmiId(format!("{}:datum-system", pmi_id(&reference.id).0));
        PmiAnnotation {
            id,
            name: None,
            targets: Vec::new(),
            definition: PmiDefinition::DatumSystem { references },
        }
    });
    let datum_system = system.as_ref().map(|system| system.id.clone());
    let (defined_unit, defined_area_unit, defined_area_second_unit) = defined_area(entity);
    Some((
        system,
        PmiAnnotation {
            id: pmi_id(&reference.id),
            name: object_name(entity),
            targets: targets(entity, feature_index),
            definition: PmiDefinition::GeometricTolerance {
                tolerance: kind,
                magnitude: length(magnitude),
                defined_unit,
                defined_area_unit,
                defined_area_second_unit,
                datum_system,
                modifiers: tolerance_modifiers(entity),
            },
        },
    ))
}

fn project_lower_profile_tier(
    reference: &Reference,
    entity: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<PmiAnnotation> {
    let magnitude = finite_nonnegative(entity.doubles.get("ToleranceLowerTier").copied()?)?;
    Some(PmiAnnotation {
        id: PmiId(format!("{}:lower-tier", pmi_id(&reference.id).0)),
        name: object_name(entity).map(|name| format!("{name} lower tier")),
        targets: targets(entity, feature_index),
        definition: PmiDefinition::GeometricTolerance {
            tolerance: GeometricToleranceKind::SurfaceProfile,
            magnitude: length(magnitude),
            defined_unit: None,
            defined_area_unit: None,
            defined_area_second_unit: None,
            datum_system: None,
            modifiers: vec!["composite_lower_tier".into()],
        },
    })
}

fn project_dimension(
    reference: &Reference,
    entity: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<PmiAnnotation> {
    let dimension = dimension_kind(short_class(&entity.class))?;
    let quantity = dimension_quantity(&dimension);
    let nominal = finite(entity.doubles.get("Nominal").copied()?)
        .filter(|value| *value != 0.0 || entity.integers.contains_key("Dimension"));
    let lower_deviation = deviation(entity, nominal, "LowerLimit", "MinusTolerance");
    let upper_deviation = deviation(entity, nominal, "UpperLimit", "PlusTolerance");
    Some(PmiAnnotation {
        id: pmi_id(&reference.id),
        name: object_name(entity),
        targets: targets(entity, feature_index),
        definition: PmiDefinition::Dimension {
            dimension,
            nominal: nominal.map(|value| pmi_value(value, quantity)),
            lower_deviation: lower_deviation.map(|value| pmi_value(value, quantity)),
            upper_deviation: upper_deviation.map(|value| pmi_value(value, quantity)),
            limits_and_fits: None,
        },
    })
}

fn enrich_implicit_nominals(
    root: &Entity,
    rendered: &[RenderedDimension],
    annotations: &mut [PmiAnnotation],
) {
    let feature_index = feature_index(root);
    for (reference, entity) in root
        .annotations
        .references
        .iter()
        .zip(&root.annotations.entities)
    {
        if suppressed(entity) {
            continue;
        }
        let (dimension_kind, source) = match short_class(&entity.class) {
            "GdtDiameter" => (
                DimensionKind::Diameter,
                diameter_from_applied_geometry(entity, &feature_index).map(|geometry| {
                    ImplicitNominal::Rendered {
                        kind: RenderedDimensionKind::Diameter,
                        geometry,
                    }
                }),
            ),
            "GdtDepth" => (DimensionKind::Size, depth_nominal(entity, &feature_index)),
            "GdtWidth" => (
                DimensionKind::Size,
                width_from_applied_geometry(entity, &feature_index).map(ImplicitNominal::Exact),
            ),
            "GdtRadius" => (
                DimensionKind::Radius,
                radius_from_applied_geometry(entity, &feature_index).map(ImplicitNominal::Exact),
            ),
            "GdtLength" => (
                DimensionKind::Size,
                length_from_applied_geometry(entity, &feature_index).map(ImplicitNominal::Exact),
            ),
            "GdtCounterBore" => (
                DimensionKind::Size,
                counterbore_from_direct_geometry(entity, &feature_index)
                    .map(ImplicitNominal::Exact),
            ),
            "GdtCounterSinkDiameter" => (
                DimensionKind::Size,
                countersink_diameter_from_direct_geometry(entity, &feature_index)
                    .map(ImplicitNominal::Exact),
            ),
            "GdtCounterSinkAngle" => (
                DimensionKind::Angular,
                countersink_angle_from_direct_geometry(entity, &feature_index)
                    .map(ImplicitNominal::Exact),
            ),
            _ => continue,
        };
        let Some(source) = source else {
            continue;
        };
        let nominal = match source {
            ImplicitNominal::Exact(value) => Some(value),
            ImplicitNominal::Rendered { kind, geometry } => entity
                .integers
                .get("BlockToleranceDecimalPlaces")
                .copied()
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| *value <= 9)
                .and_then(|decimal_places| {
                    rendered_nominal(geometry, decimal_places, kind, rendered)
                }),
            ImplicitNominal::RenderedOrExact {
                kind,
                geometry,
                exact,
            } => entity
                .integers
                .get("BlockToleranceDecimalPlaces")
                .copied()
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| *value <= 9)
                .and_then(|decimal_places| {
                    rendered_nominal(geometry, decimal_places, kind, rendered)
                })
                .or(Some(exact)),
        };
        let Some(nominal) = nominal else {
            continue;
        };
        let Some(annotation) = annotations
            .iter_mut()
            .find(|annotation| annotation.id == pmi_id(&reference.id))
        else {
            continue;
        };
        let PmiDefinition::Dimension {
            dimension,
            nominal: slot,
            lower_deviation,
            upper_deviation,
            ..
        } = &mut annotation.definition
        else {
            continue;
        };
        if dimension == &dimension_kind && slot.is_none() {
            let quantity = dimension_quantity(&dimension_kind);
            *slot = Some(pmi_value(nominal, quantity));
            *lower_deviation = deviation(entity, Some(nominal), "LowerLimit", "MinusTolerance")
                .map(|value| pmi_value(value, quantity));
            *upper_deviation = deviation(entity, Some(nominal), "UpperLimit", "PlusTolerance")
                .map(|value| pmi_value(value, quantity));
        }
    }
}

fn dimension_quantity(dimension: &DimensionKind) -> PmiQuantity {
    matches!(dimension, DimensionKind::Angular)
        .then_some(PmiQuantity::Angle)
        .unwrap_or(PmiQuantity::Length)
}

fn diameter_from_applied_geometry(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    measurement_from_applied_geometry(annotation, |id| {
        diameter_for_feature(id, feature_index, &mut BTreeSet::new(), 0)
    })
}

fn depth_from_applied_geometry(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    measurement_from_applied_geometry(annotation, |id| {
        depth_for_feature(id, feature_index, &mut BTreeSet::new(), 0)
    })
}

fn depth_nominal(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<ImplicitNominal> {
    if annotation
        .integers
        .get("IsThreadDepth")
        .is_some_and(|value| *value != 0)
    {
        return thread_depth_from_direct_geometry(annotation, feature_index)
            .map(ImplicitNominal::Exact);
    }
    if let Some(exact) = direct_cylinder_depth(annotation, feature_index) {
        return Some(ImplicitNominal::RenderedOrExact {
            kind: RenderedDimensionKind::Depth,
            geometry: exact,
            exact,
        });
    }
    depth_from_applied_geometry(annotation, feature_index).map(|geometry| {
        ImplicitNominal::Rendered {
            kind: RenderedDimensionKind::Depth,
            geometry,
        }
    })
}

fn direct_cylinder_depth(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    measurement_from_direct_features(
        annotation,
        feature_index,
        "GdtCylinder",
        nominal_cylinder_depth,
    )
}

fn thread_depth_from_direct_geometry(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    measurement_from_direct_features(annotation, feature_index, "GdtCylinder", |feature| {
        feature
            .integers
            .get("IsThreaded")
            .is_some_and(|value| *value != 0)
            .then(|| {
                feature
                    .doubles
                    .get("ThreadDepth")
                    .copied()
                    .and_then(finite_positive)
            })
            .flatten()
    })
}

fn width_from_applied_geometry(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    measurement_from_applied_geometry(annotation, |id| {
        width_for_feature(id, feature_index, &mut BTreeSet::new(), 0)
    })
}

fn radius_from_applied_geometry(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    measurement_from_applied_geometry(annotation, |id| {
        radius_for_feature(id, feature_index, &mut BTreeSet::new(), 0)
    })
}

fn length_from_applied_geometry(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    measurement_from_applied_geometry(annotation, |id| {
        length_for_feature(id, feature_index, &mut BTreeSet::new(), 0)
    })
}

fn counterbore_from_direct_geometry(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    measurement_from_direct_features(annotation, feature_index, "GdtCylinder", |feature| {
        finite_positive(nominal_radius(feature, "NomCylinder")? * 2.0)
    })
}

fn countersink_diameter_from_direct_geometry(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    measurement_from_direct_features(
        annotation,
        feature_index,
        "GdtCone",
        nominal_cone_top_diameter,
    )
}

fn countersink_angle_from_direct_geometry(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    measurement_from_direct_features(annotation, feature_index, "GdtCone", nominal_cone_angle)
}

fn measurement_from_applied_geometry(
    annotation: &Entity,
    mut measurement: impl FnMut(&str) -> Option<f64>,
) -> Option<f64> {
    let candidates = annotation
        .features
        .references
        .iter()
        .filter_map(|reference| measurement(&reference.id))
        .collect::<Vec<_>>();
    unique_measurement(&candidates)
}

fn measurement_from_direct_features(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
    class: &str,
    measurement: impl Fn(&Entity) -> Option<f64>,
) -> Option<f64> {
    let candidates = annotation
        .features
        .references
        .iter()
        .filter_map(|reference| feature_index.get(reference.id.as_str()).copied())
        .filter(|feature| short_class(&feature.class) == class)
        .filter_map(measurement)
        .collect::<Vec<_>>();
    unique_measurement(&candidates)
}

fn rendered_nominal(
    raw_mm: f64,
    decimal_places: u32,
    kind: RenderedDimensionKind,
    rendered_dimensions: &[RenderedDimension],
) -> Option<f64> {
    const LENGTH_SCALES_MM: &[f64] = &[
        1.0e-7, 1.0e-6, 1.0e-3, 0.0254, 1.0, 10.0, 25.4, 304.8, 1000.0,
    ];
    let exponent = i32::try_from(decimal_places).ok()?;
    let precision = 10.0_f64.powi(exponent);
    let mut candidates = Vec::new();
    for scale in LENGTH_SCALES_MM {
        let rendered = (raw_mm / scale * precision).round() / precision;
        for value in rendered_dimensions
            .iter()
            .filter(|value| value.kind == kind && value.decimal_places == decimal_places)
        {
            if approximately_equal(value.value, rendered) {
                candidates.push(value.value * scale);
            }
        }
    }
    unique_measurement(&candidates)
}

fn rendered_dimensions(payload: &[u8]) -> Vec<RenderedDimension> {
    const STRING_MARKER: &[u8] = &[0xff, 0xfe, 0xff];
    payload
        .windows(STRING_MARKER.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == STRING_MARKER).then_some(offset))
        .filter_map(|offset| {
            let length_offset = offset.checked_add(STRING_MARKER.len())?;
            let units = usize::from(*payload.get(length_offset)?);
            if !(1..=128).contains(&units) {
                return None;
            }
            let start = length_offset.checked_add(1)?;
            let end = start.checked_add(units.checked_mul(2)?)?;
            let code_units = payload
                .get(start..end)?
                .chunks_exact(2)
                .map(|pair| Some(u16::from_le_bytes(pair.try_into().ok()?)))
                .collect::<Option<Vec<_>>>()?;
            let text = String::from_utf16(&code_units).ok()?;
            Some(rendered_dimension_literals(&text))
        })
        .flatten()
        .collect()
}

fn rendered_dimension_literals(text: &str) -> Vec<RenderedDimension> {
    const TOKENS: &[(&str, RenderedDimensionKind)] = &[
        ("<MOD-DIAM>", RenderedDimensionKind::Diameter),
        ("&lt;MOD-DIAM&gt;", RenderedDimensionKind::Diameter),
        ("<HOLE-DEPTH>", RenderedDimensionKind::Depth),
        ("&lt;HOLE-DEPTH&gt;", RenderedDimensionKind::Depth),
    ];
    let mut values = Vec::new();
    for (token, kind) in TOKENS {
        let mut remainder = text;
        while let Some((_, tail)) = remainder.split_once(token) {
            let literal = tail.trim_start();
            let end = literal
                .bytes()
                .position(|byte| !byte.is_ascii_digit() && !matches!(byte, b'.' | b'+' | b'-'))
                .unwrap_or(literal.len());
            let literal = literal.get(..end).unwrap_or_default();
            if let Some((_, fractional)) = literal.split_once('.') {
                let parsed = literal.parse::<f64>().ok();
                let places = u32::try_from(fractional.len()).ok();
                if !fractional.is_empty() && fractional.bytes().all(|byte| byte.is_ascii_digit()) {
                    if let (Some(value), Some(decimal_places)) = (parsed, places) {
                        if value.is_finite() && value > 0.0 {
                            values.push(RenderedDimension {
                                kind: *kind,
                                value,
                                decimal_places,
                            });
                        }
                    }
                }
            }
            remainder = tail;
        }
    }
    values
}

fn diameter_for_feature(
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    visited: &mut BTreeSet<String>,
    depth: usize,
) -> Option<f64> {
    measurement_for_feature(id, feature_index, visited, depth, |feature| {
        let radius = match short_class(&feature.class) {
            "GdtCylinder" => nominal_radius(feature, "NomCylinder"),
            "GdtSphere" => nominal_radius(feature, "NomSphere"),
            _ => None,
        }?;
        finite_positive(radius * 2.0)
    })
}

fn depth_for_feature(
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    visited: &mut BTreeSet<String>,
    depth: usize,
) -> Option<f64> {
    measurement_for_feature(id, feature_index, visited, depth, |feature| {
        (short_class(&feature.class) == "GdtCylinder")
            .then(|| nominal_cylinder_depth(feature))
            .flatten()
    })
}

fn width_for_feature(
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    visited: &mut BTreeSet<String>,
    depth: usize,
) -> Option<f64> {
    measurement_for_feature(
        id,
        feature_index,
        visited,
        depth,
        |feature| match short_class(&feature.class) {
            "GdtCompoundWidth" => nominal_measurement(feature, "NomCompoundWidth", "Width"),
            "GdtCompoundClosedSlot3D" => nominal_measurement(feature, "NomClosedSlot", "Width"),
            _ => None,
        },
    )
}

fn radius_for_feature(
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    visited: &mut BTreeSet<String>,
    depth: usize,
) -> Option<f64> {
    measurement_for_feature(
        id,
        feature_index,
        visited,
        depth,
        |feature| match short_class(&feature.class) {
            "GdtFillet" => feature
                .doubles
                .get("Radius")
                .copied()
                .and_then(finite_positive),
            "GdtCylinder" => nominal_radius(feature, "NomCylinder"),
            "GdtSphere" => nominal_radius(feature, "NomSphere"),
            _ => None,
        },
    )
}

fn length_for_feature(
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    visited: &mut BTreeSet<String>,
    depth: usize,
) -> Option<f64> {
    measurement_for_feature(
        id,
        feature_index,
        visited,
        depth,
        |feature| match short_class(&feature.class) {
            "GdtCompoundClosedSlot3D" => nominal_measurement(feature, "NomClosedSlot", "Length"),
            _ => None,
        },
    )
}

fn measurement_for_feature(
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    visited: &mut BTreeSet<String>,
    depth: usize,
    direct_measurement: impl Copy + Fn(&Entity) -> Option<f64>,
) -> Option<f64> {
    if depth >= MAX_DEPTH || !visited.insert(id.to_string()) {
        return None;
    }
    let feature = feature_index.get(id)?;
    if let Some(measurement) = direct_measurement(feature) {
        return Some(measurement);
    }
    let next_depth = depth.checked_add(1)?;
    let candidates = child_feature_ids(feature)
        .into_iter()
        .filter_map(|child| {
            measurement_for_feature(
                child,
                feature_index,
                &mut visited.clone(),
                next_depth,
                direct_measurement,
            )
        })
        .collect::<Vec<_>>();
    unique_measurement(&candidates)
}

fn child_feature_ids(feature: &Entity) -> Vec<&str> {
    let mut ids = feature
        .features
        .references
        .iter()
        .map(|reference| reference.id.as_str())
        .collect::<Vec<_>>();
    if let Some(subfeatures) = direct_subfeature_ids(feature) {
        ids.extend(subfeatures);
    }
    ids
}

fn nominal_radius(feature: &Entity, name: &str) -> Option<f64> {
    nominal_measurement(feature, name, "R")
}

fn nominal_measurement(feature: &Entity, object: &str, field: &str) -> Option<f64> {
    finite_positive(
        unique_related(feature, object)?
            .entity
            .doubles
            .get(field)
            .copied()?,
    )
}

fn nominal_cylinder_depth(feature: &Entity) -> Option<f64> {
    let cylinder = &unique_related(feature, "NomCylinder")?.entity;
    let top = &unique_related(feature, "NomTop")?.entity;
    let bottom = &unique_related(feature, "NomBottom")?.entity;
    let [i, j, k] = vector(cylinder, ["I", "J", "K"])?;
    let [top_x, top_y, top_z] = vector(top, ["X", "Y", "Z"])?;
    let [bottom_x, bottom_y, bottom_z] = vector(bottom, ["X", "Y", "Z"])?;
    let axis_norm = i.hypot(j).hypot(k);
    if !approximately_equal(axis_norm, 1.0) {
        return None;
    }
    let dx = top_x - bottom_x;
    let dy = top_y - bottom_y;
    let dz = top_z - bottom_z;
    let displacement = dx.hypot(dy).hypot(dz);
    let axial = (dx * i + dy * j + dz * k).abs();
    approximately_equal(displacement, axial)
        .then_some(axial)
        .and_then(finite_positive)
}

fn nominal_cone_angle(feature: &Entity) -> Option<f64> {
    let angle = unique_related(feature, "NomCone")?
        .entity
        .doubles
        .get("FullAngle")
        .copied()
        .and_then(finite_positive)?;
    (angle < std::f64::consts::PI).then_some(angle)
}

fn nominal_cone_top_diameter(feature: &Entity) -> Option<f64> {
    let cone = &unique_related(feature, "NomCone")?.entity;
    let top = &unique_related(feature, "NomTop")?.entity;
    let angle = nominal_cone_angle(feature)?;
    let [axis_x, axis_y, axis_z] = vector(cone, ["I", "J", "K"])?;
    let [apex_x, apex_y, apex_z] = vector(cone, ["X", "Y", "Z"])?;
    let [top_i, top_j, top_k] = vector(top, ["I", "J", "K"])?;
    let [top_x, top_y, top_z] = vector(top, ["X", "Y", "Z"])?;
    if !approximately_equal(axis_x.hypot(axis_y).hypot(axis_z), 1.0)
        || !approximately_equal(top_i.hypot(top_j).hypot(top_k), 1.0)
        || !approximately_equal(
            (axis_x * top_i + axis_y * top_j + axis_z * top_k).abs(),
            1.0,
        )
    {
        return None;
    }
    let dx = top_x - apex_x;
    let dy = top_y - apex_y;
    let dz = top_z - apex_z;
    let displacement = dx.hypot(dy).hypot(dz);
    let axial = (dx * axis_x + dy * axis_y + dz * axis_z).abs();
    if !approximately_equal(displacement, axial) {
        return None;
    }
    finite_positive(axial * (angle / 2.0).tan() * 2.0)
}

fn vector<const N: usize>(entity: &Entity, names: [&str; N]) -> Option<[f64; N]> {
    let values = names.map(|name| entity.doubles.get(name).copied().and_then(finite));
    values
        .into_iter()
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

fn unique_measurement(values: &[f64]) -> Option<f64> {
    let first = *values.first()?;
    values
        .iter()
        .all(|value| approximately_equal(*value, first))
        .then_some(first)
}

fn approximately_equal(left: f64, right: f64) -> bool {
    let scale = left.abs().max(right.abs()).max(1.0);
    (left - right).abs() <= scale * 1.0e-9
}

fn deviation(
    entity: &Entity,
    nominal: Option<f64>,
    limit_key: &str,
    tolerance_key: &str,
) -> Option<f64> {
    let tolerance = finite(entity.doubles.get(tolerance_key).copied()?);
    if tolerance != Some(0.0) {
        return tolerance;
    }
    if let (Some(nominal), Some(limit)) = (
        nominal,
        entity
            .doubles
            .get(limit_key)
            .copied()
            .and_then(finite)
            .filter(|limit| *limit != 0.0),
    ) {
        return finite(limit - nominal);
    }
    tolerance
}

fn datum_references(entity: &Entity, datum_ids: &BTreeMap<&str, PmiId>) -> Vec<DatumReference> {
    let mut result = Vec::new();
    for (name, precedence) in [
        ("PrimaryDatums", 1),
        ("SecondaryDatums", 2),
        ("TertiaryDatums", 3),
    ] {
        let Some(collection) = unique_related(entity, name) else {
            continue;
        };
        let applied = collection
            .entity
            .related
            .iter()
            .filter(|object| object.class.ends_with(".GdtAppliedDatum"))
            .collect::<Vec<_>>();
        for datum in &applied {
            let [reference] = datum.entity.annotations.references.as_slice() else {
                continue;
            };
            let Some(id) = datum_ids.get(reference.id.as_str()) else {
                continue;
            };
            result.push(DatumReference {
                datum: (*id).clone(),
                precedence,
                common_group: (applied.len() > 1).then_some(precedence),
                modifiers: integer_modifier(datum.entity.integers.get("Modifier").copied()),
            });
        }
    }
    result
}

fn unique_related<'a>(entity: &'a Entity, name: &str) -> Option<&'a RelatedObject> {
    let mut matches = entity.related.iter().filter(|object| object.name == name);
    let object = matches.next()?;
    matches.next().is_none().then_some(object)
}

fn feature_index(root: &Entity) -> BTreeMap<&str, &Entity> {
    if root.features.references.len() != root.features.entities.len() {
        return BTreeMap::new();
    }
    root.features
        .references
        .iter()
        .zip(&root.features.entities)
        .map(|(reference, entity)| (reference.id.as_str(), entity))
        .collect()
}

fn targets(entity: &Entity, feature_index: &BTreeMap<&str, &Entity>) -> Vec<PmiTarget> {
    let mut ids = Vec::new();
    for reference in &entity.features.references {
        ids.extend(expanded_feature_ids(&reference.id, feature_index, 0));
    }
    let mut seen = BTreeSet::new();
    ids.into_iter()
        .filter(|id| seen.insert(id.clone()))
        .map(|source_id| PmiTarget::ShapeAspect { source_id })
        .collect()
}

fn expanded_feature_ids(
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    depth: usize,
) -> Vec<String> {
    let Some(feature) = feature_index.get(id) else {
        return vec![id.to_string()];
    };
    if short_class(&feature.class) != "GdtPattern" || depth >= MAX_DEPTH {
        return vec![id.to_string()];
    }
    let Some(subfeatures) = direct_subfeature_ids(feature) else {
        return vec![id.to_string()];
    };
    let mut members = Vec::new();
    for subfeature in subfeatures {
        let Some(next_depth) = depth.checked_add(1) else {
            return vec![id.to_string()];
        };
        members.extend(expanded_feature_ids(subfeature, feature_index, next_depth));
    }
    if members.is_empty() {
        vec![id.to_string()]
    } else {
        members
    }
}

fn direct_subfeature_ids(feature: &Entity) -> Option<Vec<&str>> {
    let collection = unique_related(feature, "SubFeatures")?;
    let mut ids = Vec::new();
    for applied in &collection.entity.related {
        if !applied.class.ends_with(".GdtAppliedFeature") {
            return None;
        }
        let [reference] = applied.entity.features.references.as_slice() else {
            return None;
        };
        ids.push(reference.id.as_str());
    }
    (!ids.is_empty()).then_some(ids)
}

fn tolerance_modifiers(entity: &Entity) -> Vec<String> {
    let mut values = integer_modifier(entity.integers.get("Modifier").copied());
    for (key, name) in [
        ("IsFreeState", "free_state"),
        ("IsStatistical", "statistical"),
        ("IsToBeInspected", "inspection"),
        ("IsTangentPlane", "tangent_plane"),
    ] {
        if entity.integers.get(key).is_some_and(|value| *value != 0) {
            values.push(name.into());
        }
    }
    if entity
        .integers
        .get("ProjectedZoneEnabled")
        .is_some_and(|value| *value != 0)
    {
        if let Some(value) = entity
            .doubles
            .get("ProjectedZoneValue")
            .and_then(|v| finite_nonnegative(*v))
        {
            values.push(format!("projected_zone:{value}_mm"));
        } else {
            values.push("projected_zone".into());
        }
    }
    if entity
        .integers
        .get("IsMaxTolerance")
        .is_some_and(|value| *value != 0)
    {
        if let Some(value) = entity
            .doubles
            .get("MaxTolerance")
            .and_then(|v| finite_nonnegative(*v))
        {
            values.push(format!("maximum_tolerance:{value}_mm"));
        } else {
            values.push("maximum_tolerance".into());
        }
    }
    values
}

fn integer_modifier(value: Option<i32>) -> Vec<String> {
    match value {
        Some(1) => vec!["maximum_material_requirement".into()],
        Some(2) => vec!["least_material_requirement".into()],
        Some(value) if value != 0 => vec![format!("sldprt:{value}")],
        _ => Vec::new(),
    }
}

fn defined_area(entity: &Entity) -> (Option<PmiValue>, Option<String>, Option<PmiValue>) {
    if entity
        .integers
        .get("PerUnitArea")
        .is_none_or(|value| *value == 0)
    {
        return (None, None, None);
    }
    match entity.integers.get("PerUnitAreaType").copied() {
        Some(0) => (
            entity
                .doubles
                .get("PerUnitAreaLength")
                .copied()
                .and_then(finite_positive)
                .map(length),
            Some("rectangular".into()),
            entity
                .doubles
                .get("PerUnitAreaWidth")
                .copied()
                .and_then(finite_positive)
                .map(length),
        ),
        Some(1) => (
            entity
                .doubles
                .get("PerUnitAreaDiameter")
                .copied()
                .and_then(finite_positive)
                .map(length),
            Some("circular".into()),
            None,
        ),
        Some(kind) => (None, Some(format!("sldprt:{kind}")), None),
        None => (None, Some("sldprt:unspecified".into()), None),
    }
}

fn tolerance_kind(class: &str) -> Option<GeometricToleranceKind> {
    use GeometricToleranceKind as Kind;
    Some(match class {
        "GdtStraightness" => Kind::Straightness,
        "GdtFlatness" => Kind::Flatness,
        "GdtRoundness" | "GdtCircularity" => Kind::Roundness,
        "GdtCylindricity" => Kind::Cylindricity,
        "GdtCoaxiality" => Kind::Coaxiality,
        "GdtLineProfile" => Kind::LineProfile,
        "GdtSurfaceProfile" => Kind::SurfaceProfile,
        "GdtCompositeSurfaceProfile" => Kind::SurfaceProfile,
        "GdtAngularity" => Kind::Angularity,
        "GdtPerpendicularity" => Kind::Perpendicularity,
        "GdtParallelism" => Kind::Parallelism,
        "GdtPosition" => Kind::Position,
        "GdtConcentricity" => Kind::Concentricity,
        "GdtSymmetry" => Kind::Symmetry,
        "GdtCircularRunout" => Kind::CircularRunout,
        "GdtTotalRunout" => Kind::TotalRunout,
        _ => return None,
    })
}

fn dimension_kind(class: &str) -> Option<DimensionKind> {
    Some(match class {
        "GdtDiameter" => DimensionKind::Diameter,
        "GdtRadius" => DimensionKind::Radius,
        "GdtAngle" | "GdtAngleBetween" | "GdtCounterSinkAngle" => DimensionKind::Angular,
        "GdtDistanceBetween" => DimensionKind::Location,
        "GdtWidth" | "GdtLength" | "GdtDepth" | "GdtCounterBore" | "GdtCounterSinkDiameter" => {
            DimensionKind::Size
        }
        _ => return None,
    })
}

fn short_class(class: &str) -> &str {
    class.rsplit('.').next().unwrap_or(class)
}

fn object_name(entity: &Entity) -> Option<String> {
    entity
        .strings
        .get("ObjectName")
        .filter(|name| !name.is_empty())
        .cloned()
}

fn pmi_id(source_id: &str) -> PmiId {
    PmiId(format!("sldprt:model:pmi#{source_id}"))
}

fn suppressed(entity: &Entity) -> bool {
    entity
        .integers
        .get("IsSuppressed")
        .is_some_and(|value| *value != 0)
}

fn finite(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
}

fn finite_nonnegative(value: f64) -> Option<f64> {
    (value.is_finite() && value >= 0.0).then_some(value)
}

fn finite_positive(value: f64) -> Option<f64> {
    (value.is_finite() && value > 0.0).then_some(value)
}

fn length(value: f64) -> PmiValue {
    pmi_value(value, PmiQuantity::Length)
}

fn pmi_value(value: f64, quantity: PmiQuantity) -> PmiValue {
    PmiValue { value, quantity }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(id: &str, class: &str) -> Reference {
        Reference {
            id: id.into(),
            class: format!("PrizMetrik.GdtAnalysis.{class},gdtanalysis.net"),
        }
    }

    fn entity(class: &str) -> Entity {
        Entity {
            class: format!("PrizMetrik.GdtAnalysis.{class}"),
            ..Entity::default()
        }
    }

    fn semantic_root() -> Entity {
        let mut datum = entity("GdtDatum");
        datum.strings.insert("ObjectName".into(), "Datum A".into());
        datum.strings.insert("DatumIdentifier".into(), "A".into());
        datum.features.references.push(reference("F10", "GdtPlane"));

        let mut applied = entity("GdtAppliedDatum");
        applied.integers.insert("Modifier".into(), 2);
        applied
            .annotations
            .references
            .push(reference("A10", "GdtDatum"));
        let mut collection = entity("GdtAppliedDatumCollection");
        collection.related.push(RelatedObject {
            name: "SubAnnotation0".into(),
            class: "PrizMetrik.GdtAnalysis.GdtAppliedDatum".into(),
            entity: applied,
        });

        let mut position = entity("GdtPosition");
        position
            .strings
            .insert("ObjectName".into(), "Position 1".into());
        position.integers.insert("Modifier".into(), 1);
        position.integers.insert("ProjectedZoneEnabled".into(), 1);
        position.doubles.insert("Tolerance".into(), 0.25);
        position.doubles.insert("ProjectedZoneValue".into(), 4.0);
        position
            .features
            .references
            .push(reference("FP", "GdtPattern"));
        position.related.push(RelatedObject {
            name: "PrimaryDatums".into(),
            class: "PrizMetrik.GdtAnalysis.GdtAppliedDatumCollection".into(),
            entity: collection,
        });

        let mut diameter = entity("GdtDiameter");
        diameter
            .strings
            .insert("ObjectName".into(), "Diameter 1".into());
        diameter.doubles.insert("Nominal".into(), 0.0);
        diameter.doubles.insert("MinusTolerance".into(), -0.1);
        diameter.doubles.insert("PlusTolerance".into(), 0.2);
        diameter.doubles.insert("LowerLimit".into(), 0.0);
        diameter.doubles.insert("UpperLimit".into(), 0.0);
        diameter
            .features
            .references
            .push(reference("F20", "GdtCylinder"));

        let mut angle = entity("GdtAngleBetween");
        angle.integers.insert("Dimension".into(), 1);
        angle.doubles.insert("Nominal".into(), 0.0);
        angle.doubles.insert("MinusTolerance".into(), -0.01);
        angle.doubles.insert("PlusTolerance".into(), 0.01);
        angle.doubles.insert("LowerLimit".into(), 0.0);
        angle.doubles.insert("UpperLimit".into(), 0.0);

        let mut root = Entity {
            class: ROOT_CLASS.into(),
            ..Entity::default()
        };
        let cylinder = entity("GdtCylinder");
        let second_cylinder = cylinder.clone();
        let mut subfeatures = entity("GdtAppliedFeatureCollection");
        for (ordinal, id) in ["F20", "F21"].into_iter().enumerate() {
            let mut applied = entity("GdtAppliedFeature");
            applied
                .features
                .references
                .push(reference(id, "GdtCylinder"));
            subfeatures.related.push(RelatedObject {
                name: format!("SubFeature{ordinal}"),
                class: "PrizMetrik.GdtAnalysis.GdtAppliedFeature".into(),
                entity: applied,
            });
        }
        let mut pattern = entity("GdtPattern");
        pattern.related.push(RelatedObject {
            name: "SubFeatures".into(),
            class: "PrizMetrik.GdtAnalysis.GdtAppliedFeatureCollection".into(),
            entity: subfeatures,
        });
        root.features.references = vec![
            reference("FP", "GdtPattern"),
            reference("F20", "GdtCylinder"),
            reference("F21", "GdtCylinder"),
        ];
        root.features.entities = vec![pattern, cylinder, second_cylinder];
        root.annotations.references = vec![
            reference("A10", "GdtDatum"),
            reference("A20", "GdtPosition"),
            reference("A30", "GdtDiameter"),
            reference("A40", "GdtAngleBetween"),
        ];
        root.annotations.entities = vec![datum, position, diameter, angle];
        root
    }

    fn put_pstr(bytes: &mut Vec<u8>, value: &str) {
        bytes.push(u8::try_from(value.len()).expect("fixture Pascal string"));
        bytes.extend_from_slice(value.as_bytes());
    }

    fn encode_entity(entity: &Entity, bytes: &mut Vec<u8>) {
        put_pstr(bytes, "Entity");
        put_pstr(bytes, &entity.class);
        put_pstr(bytes, "gdtanalysis.net");
        bytes.extend_from_slice(&1u32.to_le_bytes());
        if !entity.strings.is_empty() {
            put_pstr(bytes, "Strings");
            bytes.extend_from_slice(&(entity.strings.len() as u32).to_le_bytes());
            for (key, value) in &entity.strings {
                put_pstr(bytes, key);
                put_pstr(bytes, value);
            }
            put_pstr(bytes, "EndStrings");
        }
        if !entity.integers.is_empty() {
            put_pstr(bytes, "Integers");
            bytes.extend_from_slice(&(entity.integers.len() as u32).to_le_bytes());
            for (key, value) in &entity.integers {
                put_pstr(bytes, key);
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            put_pstr(bytes, "EndIntegers");
        }
        if !entity.doubles.is_empty() {
            put_pstr(bytes, "Doubles");
            bytes.extend_from_slice(&(entity.doubles.len() as u32).to_le_bytes());
            for (key, value) in &entity.doubles {
                put_pstr(bytes, key);
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            put_pstr(bytes, "EndDoubles");
        }
        encode_objects("Features", "EndFeatures", &entity.features, bytes);
        encode_objects("Annotations", "EndAnnotations", &entity.annotations, bytes);
        if !entity.related.is_empty() {
            put_pstr(bytes, "RelatedObjects");
            bytes.extend_from_slice(&(entity.related.len() as u32).to_le_bytes());
            for object in &entity.related {
                put_pstr(bytes, &object.name);
                put_pstr(bytes, &object.class);
            }
            for object in &entity.related {
                encode_entity(&object.entity, bytes);
            }
            put_pstr(bytes, "EndRelatedObjects");
        }
        put_pstr(bytes, "EndEntity");
    }

    fn encode_objects(name: &str, end: &str, section: &ObjectSection, bytes: &mut Vec<u8>) {
        if section.references.is_empty() && section.entities.is_empty() {
            return;
        }
        put_pstr(bytes, name);
        bytes.extend_from_slice(&(section.references.len() as u32).to_le_bytes());
        for reference in &section.references {
            put_pstr(bytes, &reference.id);
            put_pstr(bytes, &reference.class);
        }
        for entity in &section.entities {
            encode_entity(entity, bytes);
        }
        put_pstr(bytes, end);
    }

    fn encoded_root() -> Vec<u8> {
        let mut bytes = vec![0x11, 0x22, 0x33];
        encode_entity(&semantic_root(), &mut bytes);
        bytes
    }

    #[test]
    fn parses_and_projects_semantic_graph() {
        let parsed = parse_unique_root(&encoded_root()).expect("synthetic SWIFT root");
        let annotations = project(&parsed);
        assert_eq!(annotations.len(), 5);

        let position = annotations
            .iter()
            .find(|annotation| annotation.name.as_deref() == Some("Position 1"))
            .expect("position annotation");
        let PmiDefinition::GeometricTolerance {
            magnitude,
            datum_system,
            modifiers,
            ..
        } = &position.definition
        else {
            panic!("position definition");
        };
        assert_eq!(*magnitude, length(0.25));
        assert_eq!(
            datum_system.as_ref().map(|id| id.0.as_str()),
            Some("sldprt:model:pmi#A20:datum-system")
        );
        assert_eq!(
            modifiers,
            &["maximum_material_requirement", "projected_zone:4_mm"]
        );
        assert_eq!(
            position.targets,
            [
                PmiTarget::ShapeAspect {
                    source_id: "F20".into()
                },
                PmiTarget::ShapeAspect {
                    source_id: "F21".into()
                }
            ]
        );

        let system = annotations
            .iter()
            .find(|annotation| annotation.id.0.ends_with(":datum-system"))
            .expect("datum system");
        let PmiDefinition::DatumSystem { references } = &system.definition else {
            panic!("datum-system definition");
        };
        assert_eq!(references.len(), 1);
        let datum_reference = references.first().expect("primary datum reference");
        assert_eq!(datum_reference.precedence, 1);
        assert_eq!(datum_reference.modifiers, ["least_material_requirement"]);

        let diameter = annotations
            .iter()
            .find(|annotation| annotation.name.as_deref() == Some("Diameter 1"))
            .expect("diameter annotation");
        let PmiDefinition::Dimension {
            nominal,
            lower_deviation,
            upper_deviation,
            ..
        } = &diameter.definition
        else {
            panic!("diameter definition");
        };
        assert_eq!(*nominal, None, "zero is an omitted nominal sentinel");
        assert_eq!(*lower_deviation, Some(length(-0.1)));
        assert_eq!(*upper_deviation, Some(length(0.2)));

        let angle = annotations
            .iter()
            .find(|annotation| annotation.id.0.ends_with("#A40"))
            .expect("angular annotation");
        let PmiDefinition::Dimension { nominal, .. } = &angle.definition else {
            panic!("angular definition");
        };
        assert_eq!(
            *nominal,
            Some(PmiValue {
                value: 0.0,
                quantity: PmiQuantity::Angle,
            })
        );
    }

    #[test]
    fn rejects_ambiguous_root_and_impossible_count() {
        let encoded = encoded_root();
        let mut duplicate = encoded.clone();
        duplicate.extend_from_slice(&encoded);
        assert_eq!(parse_unique_root(&duplicate), None);

        let mut malformed = encoded;
        let marker = b"\x0bAnnotations";
        let offset = malformed
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("root annotations marker")
            + marker.len();
        malformed
            .get_mut(offset..offset + 4)
            .expect("count field")
            .copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(parse_unique_root(&malformed), None);
    }

    fn cylinder_with_radius(radius: f64) -> Entity {
        let mut cylinder = entity("GdtCylinder");
        let mut geometry = Entity {
            class: "PrizMetrik.Geometry.GeoCylinder".into(),
            ..Entity::default()
        };
        geometry.doubles.insert("R".into(), radius);
        geometry.doubles.insert("I".into(), 0.0);
        geometry.doubles.insert("J".into(), 0.0);
        geometry.doubles.insert("K".into(), 1.0);
        cylinder.related.push(RelatedObject {
            name: "NomCylinder".into(),
            class: geometry.class.clone(),
            entity: geometry,
        });
        cylinder
    }

    fn cylinder_with_radius_and_depth(radius: f64, depth: f64) -> Entity {
        let mut cylinder = cylinder_with_radius(radius);
        for (name, z) in [("NomTop", depth), ("NomBottom", 0.0)] {
            let mut plane = Entity {
                class: "PrizMetrik.Geometry.GeoPlane".into(),
                ..Entity::default()
            };
            plane.doubles.insert("X".into(), 0.0);
            plane.doubles.insert("Y".into(), 0.0);
            plane.doubles.insert("Z".into(), z);
            cylinder.related.push(RelatedObject {
                name: name.into(),
                class: plane.class.clone(),
                entity: plane,
            });
        }
        cylinder
    }

    fn cone_with_angle_and_top(angle: f64, top: f64) -> Entity {
        let mut cone = entity("GdtCone");
        let mut geometry = Entity {
            class: "PrizMetrik.Geometry.GeoCone".into(),
            ..Entity::default()
        };
        for (name, value) in [
            ("FullAngle", angle),
            ("I", 0.0),
            ("J", 0.0),
            ("K", 1.0),
            ("X", 0.0),
            ("Y", 0.0),
            ("Z", 0.0),
        ] {
            geometry.doubles.insert(name.into(), value);
        }
        cone.related.push(RelatedObject {
            name: "NomCone".into(),
            class: geometry.class.clone(),
            entity: geometry,
        });
        let mut plane = Entity {
            class: "PrizMetrik.Geometry.GeoPlane".into(),
            ..Entity::default()
        };
        for (name, value) in [
            ("I", 0.0),
            ("J", 0.0),
            ("K", -1.0),
            ("X", 0.0),
            ("Y", 0.0),
            ("Z", top),
        ] {
            plane.doubles.insert(name.into(), value);
        }
        cone.related.push(RelatedObject {
            name: "NomTop".into(),
            class: plane.class.clone(),
            entity: plane,
        });
        cone
    }

    fn feature_with_nominal_measurement(
        feature_class: &str,
        object_name: &str,
        geometry_class: &str,
        field: &str,
        value: f64,
    ) -> Entity {
        let mut feature = entity(feature_class);
        let mut geometry = Entity {
            class: format!("PrizMetrik.Geometry.{geometry_class}"),
            ..Entity::default()
        };
        geometry.doubles.insert(field.into(), value);
        feature.related.push(RelatedObject {
            name: object_name.into(),
            class: geometry.class.clone(),
            entity: geometry,
        });
        feature
    }

    #[test]
    fn rendered_diameter_resolves_rounded_applied_geometry() {
        let mut root = semantic_root();
        *root.features.entities.get_mut(1).expect("first cylinder") =
            cylinder_with_radius(1.984_375);
        *root.features.entities.get_mut(2).expect("second cylinder") =
            cylinder_with_radius(1.984_375);
        let diameter = root.annotations.entities.get_mut(2).expect("diameter");
        diameter
            .integers
            .insert("BlockToleranceDecimalPlaces".into(), 3);
        diameter.doubles.insert("MinusTolerance".into(), 0.0);
        diameter.doubles.insert("PlusTolerance".into(), 0.0);
        diameter.doubles.insert("LowerLimit".into(), 3.8);
        diameter.doubles.insert("UpperLimit".into(), 4.1);
        let displayed = [RenderedDimension {
            kind: RenderedDimensionKind::Diameter,
            value: 0.156,
            decimal_places: 3,
        }];
        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &displayed, &mut annotations);
        let diameter = annotations
            .iter()
            .find(|annotation| annotation.name.as_deref() == Some("Diameter 1"))
            .expect("diameter annotation");
        let PmiDefinition::Dimension {
            nominal,
            lower_deviation,
            upper_deviation,
            ..
        } = &diameter.definition
        else {
            panic!("dimension definition");
        };
        assert!(nominal
            .as_ref()
            .is_some_and(|value| approximately_equal(value.value, 3.962_4)));
        assert!(lower_deviation
            .as_ref()
            .is_some_and(|value| approximately_equal(value.value, -0.162_4)));
        assert!(upper_deviation
            .as_ref()
            .is_some_and(|value| approximately_equal(value.value, 0.137_6)));

        root.annotations
            .entities
            .get_mut(2)
            .expect("diameter")
            .features
            .references = vec![reference("FP", "GdtPattern")];
        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &displayed, &mut annotations);
        let diameter = annotations
            .iter()
            .find(|annotation| annotation.name.as_deref() == Some("Diameter 1"))
            .expect("diameter annotation");
        let PmiDefinition::Dimension { nominal, .. } = &diameter.definition else {
            panic!("dimension definition");
        };
        assert!(nominal
            .as_ref()
            .is_some_and(|value| approximately_equal(value.value, 3.962_4)));
    }

    #[test]
    fn conflicting_pattern_sizes_do_not_resolve_a_nominal() {
        let mut root = semantic_root();
        *root.features.entities.get_mut(1).expect("first cylinder") = cylinder_with_radius(2.5);
        *root.features.entities.get_mut(2).expect("second cylinder") = cylinder_with_radius(3.0);
        let diameter = root.annotations.entities.get_mut(2).expect("diameter");
        diameter
            .integers
            .insert("BlockToleranceDecimalPlaces".into(), 1);
        diameter.features.references = vec![reference("FP", "GdtPattern")];
        let mut annotations = project(&root);
        enrich_implicit_nominals(
            &root,
            &[RenderedDimension {
                kind: RenderedDimensionKind::Diameter,
                value: 5.0,
                decimal_places: 1,
            }],
            &mut annotations,
        );
        let diameter = annotations
            .iter()
            .find(|annotation| annotation.name.as_deref() == Some("Diameter 1"))
            .expect("diameter annotation");
        let PmiDefinition::Dimension { nominal, .. } = &diameter.definition else {
            panic!("dimension definition");
        };
        assert_eq!(*nominal, None);
    }

    #[test]
    fn conflicting_rendered_units_do_not_resolve_a_nominal() {
        assert_eq!(
            rendered_nominal(
                5.0,
                1,
                RenderedDimensionKind::Diameter,
                &[
                    RenderedDimension {
                        kind: RenderedDimensionKind::Diameter,
                        value: 5.0,
                        decimal_places: 1,
                    },
                    RenderedDimension {
                        kind: RenderedDimensionKind::Diameter,
                        value: 0.2,
                        decimal_places: 1,
                    },
                ],
            ),
            None
        );
    }

    #[test]
    fn rendered_depth_resolves_axial_nominal_planes() {
        let mut root = semantic_root();
        *root.features.entities.get_mut(1).expect("first cylinder") =
            cylinder_with_radius_and_depth(2.5, 7.625);
        let mut depth = entity("GdtDepth");
        depth.strings.insert("ObjectName".into(), "Depth 1".into());
        depth
            .integers
            .insert("BlockToleranceDecimalPlaces".into(), 2);
        depth.doubles.insert("Nominal".into(), 0.0);
        depth.doubles.insert("MinusTolerance".into(), -0.254);
        depth.doubles.insert("PlusTolerance".into(), 0.254);
        depth.doubles.insert("LowerLimit".into(), 0.0);
        depth.doubles.insert("UpperLimit".into(), 0.0);
        depth
            .features
            .references
            .push(reference("F20", "GdtCylinder"));
        root.annotations
            .references
            .push(reference("A50", "GdtDepth"));
        root.annotations.entities.push(depth);

        let mut annotations = project(&root);
        enrich_implicit_nominals(
            &root,
            &[RenderedDimension {
                kind: RenderedDimensionKind::Depth,
                value: 0.3,
                decimal_places: 2,
            }],
            &mut annotations,
        );
        let depth = annotations
            .iter()
            .find(|annotation| annotation.name.as_deref() == Some("Depth 1"))
            .expect("depth annotation");
        let PmiDefinition::Dimension { nominal, .. } = &depth.definition else {
            panic!("dimension definition");
        };
        assert!(nominal
            .as_ref()
            .is_some_and(|value| approximately_equal(value.value, 7.62)));
    }

    #[test]
    fn direct_and_thread_cylinders_supply_depth_without_rendered_text() {
        let mut root = semantic_root();
        *root.features.entities.get_mut(1).expect("direct cylinder") =
            cylinder_with_radius_and_depth(5.0, 14.2875);
        let mut depth = entity("GdtDepth");
        depth.doubles.insert("Nominal".into(), 0.0);
        depth
            .features
            .references
            .push(reference("F20", "GdtCylinder"));
        root.annotations
            .references
            .push(reference("A50", "GdtDepth"));
        root.annotations.entities.push(depth);

        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        let PmiDefinition::Dimension { nominal, .. } = &annotations
            .iter()
            .find(|annotation| annotation.id == pmi_id("A50"))
            .expect("direct depth annotation")
            .definition
        else {
            panic!("dimension definition");
        };
        assert!(nominal
            .as_ref()
            .is_some_and(|value| approximately_equal(value.value, 14.2875)));

        root.annotations
            .entities
            .last_mut()
            .expect("thread-depth annotation")
            .integers
            .insert("IsThreadDepth".into(), 1);
        let cylinder = root
            .features
            .entities
            .get_mut(1)
            .expect("threaded cylinder");
        cylinder.integers.insert("IsThreaded".into(), 1);
        cylinder.doubles.insert("ThreadDepth".into(), 12.0);
        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        let PmiDefinition::Dimension { nominal, .. } = &annotations
            .iter()
            .find(|annotation| annotation.id == pmi_id("A50"))
            .expect("thread depth annotation")
            .definition
        else {
            panic!("dimension definition");
        };
        assert_eq!(*nominal, Some(length(12.0)));
    }

    #[test]
    fn semantic_slot_dimensions_resolve_exact_nominals() {
        let mut root = semantic_root();
        root.features
            .references
            .push(reference("FW", "GdtCompoundWidth"));
        root.features
            .entities
            .push(feature_with_nominal_measurement(
                "GdtCompoundWidth",
                "NomCompoundWidth",
                "GeoOpenSlot",
                "Width",
                12.7,
            ));
        root.features
            .references
            .push(reference("FL", "GdtCompoundClosedSlot3D"));
        root.features
            .entities
            .push(feature_with_nominal_measurement(
                "GdtCompoundClosedSlot3D",
                "NomClosedSlot",
                "GeoClosedSlot",
                "Length",
                38.1,
            ));
        let mut width = entity("GdtWidth");
        width.strings.insert("ObjectName".into(), "Width 1".into());
        width.doubles.insert("Nominal".into(), 0.0);
        width.doubles.insert("MinusTolerance".into(), 0.0);
        width.doubles.insert("PlusTolerance".into(), 0.0);
        width.doubles.insert("LowerLimit".into(), 12.5);
        width.doubles.insert("UpperLimit".into(), 12.9);
        width
            .features
            .references
            .push(reference("FW", "GdtCompoundWidth"));
        root.annotations
            .references
            .push(reference("A50", "GdtWidth"));
        root.annotations.entities.push(width);
        let mut length = entity("GdtLength");
        length
            .strings
            .insert("ObjectName".into(), "Length 1".into());
        length.doubles.insert("Nominal".into(), 0.0);
        length
            .features
            .references
            .push(reference("FL", "GdtCompoundClosedSlot3D"));
        root.annotations
            .references
            .push(reference("A60", "GdtLength"));
        root.annotations.entities.push(length);

        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        let width = annotations
            .iter()
            .find(|annotation| annotation.name.as_deref() == Some("Width 1"))
            .expect("width annotation");
        let PmiDefinition::Dimension {
            nominal,
            lower_deviation,
            upper_deviation,
            ..
        } = &width.definition
        else {
            panic!("dimension definition");
        };
        assert!(nominal
            .as_ref()
            .is_some_and(|value| approximately_equal(value.value, 12.7)));
        assert!(lower_deviation
            .as_ref()
            .is_some_and(|value| approximately_equal(value.value, -0.2)));
        assert!(upper_deviation
            .as_ref()
            .is_some_and(|value| approximately_equal(value.value, 0.2)));
        let length = annotations
            .iter()
            .find(|annotation| annotation.name.as_deref() == Some("Length 1"))
            .expect("length annotation");
        let PmiDefinition::Dimension { nominal, .. } = &length.definition else {
            panic!("dimension definition");
        };
        assert!(nominal
            .as_ref()
            .is_some_and(|value| approximately_equal(value.value, 38.1)));
    }

    #[test]
    fn compound_hole_dimensions_use_direct_operation_geometry() {
        let mut root = semantic_root();
        *root
            .features
            .entities
            .get_mut(1)
            .expect("first pattern member") = cylinder_with_radius(3.0);
        *root
            .features
            .entities
            .get_mut(2)
            .expect("second pattern member") = cylinder_with_radius(3.0);
        root.features
            .references
            .push(reference("FCB", "GdtCylinder"));
        root.features.entities.push(cylinder_with_radius(7.9375));
        root.features.references.push(reference("FCS", "GdtCone"));
        root.features
            .entities
            .push(cone_with_angle_and_top(std::f64::consts::FRAC_PI_2, 10.0));

        for (id, class, feature) in [
            ("ACB", "GdtCounterBore", "FCB"),
            ("ACSD", "GdtCounterSinkDiameter", "FCS"),
            ("ACSA", "GdtCounterSinkAngle", "FCS"),
        ] {
            let mut annotation = entity(class);
            annotation.doubles.insert("Nominal".into(), 0.0);
            annotation
                .features
                .references
                .push(reference("FP", "GdtPattern"));
            annotation.features.references.push(reference(
                feature,
                if feature == "FCB" {
                    "GdtCylinder"
                } else {
                    "GdtCone"
                },
            ));
            root.annotations.references.push(reference(id, class));
            root.annotations.entities.push(annotation);
        }

        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        for (id, expected, quantity) in [
            ("ACB", 15.875, PmiQuantity::Length),
            ("ACSD", 20.0, PmiQuantity::Length),
            ("ACSA", std::f64::consts::FRAC_PI_2, PmiQuantity::Angle),
        ] {
            let PmiDefinition::Dimension { nominal, .. } = &annotations
                .iter()
                .find(|annotation| annotation.id == pmi_id(id))
                .expect("compound-hole annotation")
                .definition
            else {
                panic!("dimension definition");
            };
            assert!(nominal.as_ref().is_some_and(|value| {
                value.quantity == quantity && approximately_equal(value.value, expected)
            }));
        }
    }

    #[test]
    fn semantic_slot_width_traverses_patterns_and_rejects_disagreement() {
        let mut root = semantic_root();
        for (index, value) in [(1, 9.525), (2, 9.525)] {
            *root
                .features
                .entities
                .get_mut(index)
                .expect("pattern member") = feature_with_nominal_measurement(
                "GdtCompoundClosedSlot3D",
                "NomClosedSlot",
                "GeoClosedSlot",
                "Width",
                value,
            );
        }
        let pattern_members = root
            .features
            .entities
            .first_mut()
            .and_then(|pattern| pattern.related.first_mut())
            .expect("pattern members");
        for applied in &mut pattern_members.entity.related {
            applied
                .entity
                .features
                .references
                .first_mut()
                .expect("pattern member reference")
                .class = "PrizMetrik.GdtAnalysis.GdtCompoundClosedSlot3D,gdtanalysis.net".into();
        }
        let mut width = entity("GdtWidth");
        width.doubles.insert("Nominal".into(), 0.0);
        width
            .features
            .references
            .push(reference("FP", "GdtPattern"));
        root.annotations
            .references
            .push(reference("A50", "GdtWidth"));
        root.annotations.entities.push(width);

        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        let PmiDefinition::Dimension { nominal, .. } = &annotations
            .iter()
            .find(|annotation| annotation.id == pmi_id("A50"))
            .expect("width annotation")
            .definition
        else {
            panic!("dimension definition");
        };
        assert!(nominal
            .as_ref()
            .is_some_and(|value| approximately_equal(value.value, 9.525)));

        root.features
            .entities
            .get_mut(2)
            .and_then(|feature| feature.related.first_mut())
            .expect("second nominal slot")
            .entity
            .doubles
            .insert("Width".into(), 6.35);
        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        let PmiDefinition::Dimension { nominal, .. } = &annotations
            .iter()
            .find(|annotation| annotation.id == pmi_id("A50"))
            .expect("width annotation")
            .definition
        else {
            panic!("dimension definition");
        };
        assert_eq!(*nominal, None);
    }

    #[test]
    fn semantic_radius_resolves_fillets_cylinders_and_spheres() {
        let mut root = semantic_root();
        let mut fillet = entity("GdtFillet");
        fillet.doubles.insert("Radius".into(), 3.175);
        *root.features.entities.get_mut(1).expect("first member") = fillet;
        *root.features.entities.get_mut(2).expect("second member") = cylinder_with_radius(3.175);
        root.features
            .references
            .get_mut(1)
            .expect("first feature reference")
            .class = "PrizMetrik.GdtAnalysis.GdtFillet,gdtanalysis.net".into();
        let pattern_members = root
            .features
            .entities
            .first_mut()
            .and_then(|pattern| pattern.related.first_mut())
            .expect("pattern members");
        for (applied, class) in pattern_members
            .entity
            .related
            .iter_mut()
            .zip(["GdtFillet", "GdtCylinder"])
        {
            applied
                .entity
                .features
                .references
                .first_mut()
                .expect("pattern member reference")
                .class = format!("PrizMetrik.GdtAnalysis.{class},gdtanalysis.net");
        }
        let mut radius = entity("GdtRadius");
        radius.doubles.insert("Nominal".into(), 0.0);
        radius.doubles.insert("MinusTolerance".into(), -0.1);
        radius.doubles.insert("PlusTolerance".into(), 0.0);
        radius.doubles.insert("UpperLimit".into(), 0.0);
        radius
            .features
            .references
            .push(reference("FP", "GdtPattern"));
        root.annotations
            .references
            .push(reference("A50", "GdtRadius"));
        root.annotations.entities.push(radius);

        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        let PmiDefinition::Dimension {
            nominal,
            upper_deviation,
            ..
        } = &annotations
            .iter()
            .find(|annotation| annotation.id == pmi_id("A50"))
            .expect("radius annotation")
            .definition
        else {
            panic!("dimension definition");
        };
        assert!(nominal
            .as_ref()
            .is_some_and(|value| approximately_equal(value.value, 3.175)));
        assert_eq!(*upper_deviation, Some(length(0.0)));

        *root.features.entities.get_mut(2).expect("second member") =
            feature_with_nominal_measurement("GdtSphere", "NomSphere", "GeoSphere", "R", 4.0);
        root.features
            .references
            .get_mut(2)
            .expect("second feature reference")
            .class = "PrizMetrik.GdtAnalysis.GdtSphere,gdtanalysis.net".into();
        root.features
            .entities
            .first_mut()
            .and_then(|pattern| pattern.related.first_mut())
            .and_then(|members| members.entity.related.get_mut(1))
            .and_then(|applied| applied.entity.features.references.first_mut())
            .expect("second pattern member reference")
            .class = "PrizMetrik.GdtAnalysis.GdtSphere,gdtanalysis.net".into();
        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        let PmiDefinition::Dimension { nominal, .. } = &annotations
            .iter()
            .find(|annotation| annotation.id == pmi_id("A50"))
            .expect("radius annotation")
            .definition
        else {
            panic!("dimension definition");
        };
        assert_eq!(*nominal, None);
    }

    #[test]
    fn scans_explicit_rendered_diameter_literals() {
        let mut payload = Vec::new();
        for text in [
            "<COUNT=#X ><MOD-DIAM> .156",
            "<MOD-DIAM> <sft_holeDia>",
            "<MOD-DIAM> .281<HOLE-SPOT><MOD-DIAM> .438",
            "<MOD-DIAM> .250 <HOLE-DEPTH> .30",
        ] {
            payload.extend_from_slice(&[0xff, 0xfe, 0xff]);
            payload.push(u8::try_from(text.encode_utf16().count()).expect("fixture length"));
            for unit in text.encode_utf16() {
                payload.extend_from_slice(&unit.to_le_bytes());
            }
        }
        assert_eq!(
            rendered_dimensions(&payload),
            [
                RenderedDimension {
                    kind: RenderedDimensionKind::Diameter,
                    value: 0.156,
                    decimal_places: 3,
                },
                RenderedDimension {
                    kind: RenderedDimensionKind::Diameter,
                    value: 0.281,
                    decimal_places: 3,
                },
                RenderedDimension {
                    kind: RenderedDimensionKind::Diameter,
                    value: 0.438,
                    decimal_places: 3,
                },
                RenderedDimension {
                    kind: RenderedDimensionKind::Diameter,
                    value: 0.25,
                    decimal_places: 3,
                },
                RenderedDimension {
                    kind: RenderedDimensionKind::Depth,
                    value: 0.3,
                    decimal_places: 2,
                },
            ]
        );
    }
}
