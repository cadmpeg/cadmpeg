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

/// Decode the unique GDT-analysis root carried by a SWIFT schema stream.
pub(crate) fn annotations(
    scan: &ContainerScan<'_>,
    annotations: &mut Annotations,
) -> Vec<PmiAnnotation> {
    let Some((stream, root)) = scan_root(scan) else {
        return Vec::new();
    };
    let projected = project(&root);
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
    let Some((_, root)) = scan_root(scan) else {
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

fn scan_root(scan: &ContainerScan<'_>) -> Option<(String, Entity)> {
    let mut roots = scan
        .sections()
        .filter(|section| {
            section
                .name()
                .is_some_and(|name| name.starts_with("SWIFT/") && name.contains("Schema"))
        })
        .filter_map(|section| {
            parse_unique_root(section.payload()).map(|root| (section.display_name(), root))
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
    let quantity = matches!(dimension, DimensionKind::Angular)
        .then_some(PmiQuantity::Angle)
        .unwrap_or(PmiQuantity::Length);
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
    if let (Some(nominal), Some(limit)) = (nominal, entity.doubles.get(limit_key).copied()) {
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
}
