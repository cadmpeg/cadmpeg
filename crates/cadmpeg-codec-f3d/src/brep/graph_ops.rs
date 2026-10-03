// SPDX-License-Identifier: Apache-2.0
//! Select and qualify typed BREP rows through an admitted structural projection.

use super::Brep;
use crate::records::recipes::CreationTimestamp;
use crate::records::sketch_links::{PersistentDesignLink, PersistentSubentityTag, SketchCurveLink};
use cadmpeg_asm::brep::AsmBrep;
use cadmpeg_asm::ids::IdFormat;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use cadmpeg_ir::schema::structural::{project, Projection};
use serde::Serialize;
use serde_value::Value;
use std::collections::HashSet;

pub(super) mod ordered;

use cadmpeg_ir::ids::comparison::{compare, equal};
use ordered::position;

pub(in crate::brep) struct AdjacencyRow {
    pub(in crate::brep) source: String,
    pub(in crate::brep) targets: Vec<String>,
}

fn contains(
    ctx: &DecodeContext<'_>,
    values: &[String],
    value: &str,
    operation: &'static str,
) -> Result<bool, CodecError> {
    ctx.charge_work(0, operation)?;
    Ok(position(ctx, values, value, String::as_str)?.is_ok())
}

fn insert_id(
    ctx: &DecodeContext<'_>,
    values: &mut Vec<String>,
    value: String,
    operation: &'static str,
) -> Result<bool, CodecError> {
    ctx.charge_work(0, operation)?;
    let Err(index) = position(ctx, values, &value, String::as_str)? else {
        return Ok(false);
    };
    ctx.reserve_vec(values, 1, operation)?;
    ctx.charge_work(
        u64_from_index(values.len() - index),
        "move F3D BREP graph IDs",
    )?;
    values.insert(index, value);
    Ok(true)
}

fn entity_id<'a>(
    ctx: &DecodeContext<'_>,
    value: &'a Value,
    operation: &'static str,
) -> Result<Option<&'a str>, CodecError> {
    ctx.charge_work(0, operation)?;
    let Value::Map(fields) = value else {
        return Ok(None);
    };
    for (key, value) in fields {
        ctx.charge_work(1, operation)?;
        if let Value::String(key) = key {
            if equal(ctx, key, "id", operation)? {
                return text_value(ctx, value, operation);
            }
        }
    }
    Ok(None)
}

fn text_value<'a>(
    ctx: &DecodeContext<'_>,
    value: &'a Value,
    operation: &'static str,
) -> Result<Option<&'a str>, CodecError> {
    let _depth = ctx.enter_nested(operation)?;
    ctx.charge_work(1, operation)?;
    match value {
        Value::String(text) => Ok(Some(text)),
        Value::Newtype(value) => text_value(ctx, value, operation),
        _ => Ok(None),
    }
}

pub(super) fn collect_owned_ids(
    ctx: &DecodeContext<'_>,
    value: &Value,
    owned: &mut Vec<String>,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("walk F3D BREP owned IDs")?;
    ctx.charge_work(1, "walk F3D BREP owned IDs")?;
    if let Some(id) = entity_id(ctx, value, "walk F3D BREP owned IDs")? {
        if !contains(ctx, owned, id, "find F3D BREP owned ID")? {
            let id = ctx.copy_retained_text(id, "copy F3D BREP owned ID")?;
            insert_id(ctx, owned, id, "index F3D BREP owned IDs")?;
        }
    }
    match value {
        Value::Map(fields) => {
            for (key, item) in fields {
                collect_owned_ids(ctx, key, owned)?;
                collect_owned_ids(ctx, item, owned)?;
            }
        }
        Value::Seq(items) => {
            for item in items {
                collect_owned_ids(ctx, item, owned)?;
            }
        }
        Value::Option(Some(item)) | Value::Newtype(item) => collect_owned_ids(ctx, item, owned)?,
        _ => {}
    }
    Ok(())
}

pub(super) fn collect_brep_references(
    ctx: &DecodeContext<'_>,
    value: &Value,
    owned: &[String],
    references: &mut Vec<String>,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("walk F3D BREP references")?;
    ctx.charge_work(1, "walk F3D BREP references")?;
    match value {
        Value::String(id) => {
            if contains(ctx, owned, id, "walk F3D BREP references")?
                && !contains(ctx, references, id, "walk F3D BREP references")?
            {
                let id = ctx.copy_retained_text(id, "copy F3D BREP adjacency reference")?;
                insert_id(ctx, references, id, "collect F3D BREP adjacency references")?;
            }
        }
        Value::Map(fields) => {
            for (key, item) in fields {
                collect_brep_references(ctx, key, owned, references)?;
                collect_brep_references(ctx, item, owned, references)?;
            }
        }
        Value::Seq(items) => {
            for item in items {
                collect_brep_references(ctx, item, owned, references)?;
            }
        }
        Value::Option(Some(item)) | Value::Newtype(item) => {
            collect_brep_references(ctx, item, owned, references)?;
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn insert_brep_adjacency(
    ctx: &DecodeContext<'_>,
    adjacency: &mut Vec<AdjacencyRow>,
    source: &str,
    target: &str,
) -> Result<(), CodecError> {
    ctx.charge_work(0, "index F3D BREP adjacency")?;
    let source_index = match position(ctx, adjacency, source, |row| row.source.as_str())? {
        Ok(index) => index,
        Err(index) => {
            let source = ctx.copy_retained_text(source, "copy F3D BREP adjacency source")?;
            ctx.reserve_vec(adjacency, 1, "index F3D BREP adjacency")?;
            ctx.charge_work(
                u64_from_index(adjacency.len() - index),
                "move F3D BREP adjacency rows",
            )?;
            adjacency.insert(
                index,
                AdjacencyRow {
                    source,
                    targets: Vec::new(),
                },
            );
            index
        }
    };
    let mut query_storage = ctx.reserve_scoped(0, "copy F3D BREP adjacency query")?;
    let query = query_storage
        .with_storage(|| ctx.copy_retained_text(source, "copy F3D BREP adjacency query"))?;
    if position(ctx, adjacency, &query, |row| row.source.as_str())? != Ok(source_index) {
        return Err(CodecError::malformed("BREP adjacency source is absent"));
    }
    let targets = &mut adjacency[source_index].targets;
    if !contains(ctx, targets, target, "find F3D BREP adjacent ID")? {
        let target = ctx.copy_retained_text(target, "copy F3D BREP adjacency target")?;
        insert_id(ctx, targets, target, "collect F3D BREP adjacent IDs")?;
    }
    Ok(())
}

fn adjacency(
    ctx: &DecodeContext<'_>,
    value: &Value,
    owned: &[String],
) -> Result<Vec<AdjacencyRow>, CodecError> {
    let mut adjacency = Vec::new();
    let Value::Map(fields) = value else {
        return Err(CodecError::malformed("BREP projection must be an object"));
    };
    for value in fields.values() {
        ctx.charge_work(1, "walk F3D BREP arenas")?;
        let Value::Seq(items) = value else {
            continue;
        };
        for item in items {
            ctx.charge_work(1, "walk F3D BREP adjacency rows")?;
            let Some(id) = entity_id(ctx, item, "find F3D BREP adjacency owner")? else {
                continue;
            };
            let mut references = Vec::new();
            collect_brep_references(ctx, item, owned, &mut references)?;
            ctx.charge_work(0, "remove F3D BREP self reference")?;
            if let Ok(index) = position(ctx, &references, id, String::as_str)? {
                ctx.charge_work(
                    u64_from_index(references.len() - index - 1),
                    "remove F3D BREP self reference",
                )?;
                references.remove(index);
            }
            ctx.charge_work(0, "remove F3D BREP self reference")?;
            for reference in references {
                ctx.charge_work(1, "walk F3D BREP adjacency references")?;
                insert_brep_adjacency(ctx, &mut adjacency, id, reference.as_ref())?;
                insert_brep_adjacency(ctx, &mut adjacency, reference.as_ref(), id)?;
            }
        }
    }
    Ok(adjacency)
}

fn projection<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    brep: &Brep,
    include_links: bool,
    operation: &'static str,
) -> Result<Projection<'ctx>, CodecError> {
    #[derive(Serialize)]
    struct View<'a> {
        #[serde(flatten)]
        asm: &'a AsmBrep,
        sketch_curve_links: &'a [SketchCurveLink],
        persistent_design_links: &'a [PersistentDesignLink],
        persistent_subentity_tags: &'a [PersistentSubentityTag],
        creation_timestamps: &'a [CreationTimestamp],
    }
    let stats = &brep.asm.stats;
    for count in [
        stats.missing_face_surface_kinds.len(),
        stats.unknown_surface_kinds.len(),
        stats.procedural_curve_kinds.len(),
        stats.undecoded_pcurve_kinds.len(),
        stats.other_record_kinds.len(),
    ] {
        ctx.charge_work(u64_from_index(count), "sum F3D BREP statistics")?;
    }
    project(
        ctx,
        &View {
            asm: &brep.asm,
            sketch_curve_links: if include_links {
                &brep.sketch_curve_links
            } else {
                &[]
            },
            persistent_design_links: if include_links {
                &brep.persistent_design_links
            } else {
                &[]
            },
            persistent_subentity_tags: if include_links {
                &brep.persistent_subentity_tags
            } else {
                &[]
            },
            creation_timestamps: if include_links {
                &brep.creation_timestamps
            } else {
                &[]
            },
        },
        operation,
    )
}

fn arena<'a>(
    ctx: &DecodeContext<'_>,
    value: &'a Value,
    name: &str,
) -> Result<&'a [Value], CodecError> {
    ctx.charge_work(0, "find F3D retained arena")?;
    let Value::Map(fields) = value else {
        return Err(CodecError::malformed("BREP projection must be an object"));
    };
    for (key, value) in fields {
        ctx.charge_work(1, "find F3D retained arena")?;
        if let Value::String(key) = key {
            if equal(ctx, key, name, "find F3D retained arena")? {
                return match value {
                    Value::Seq(items) => Ok(items),
                    _ => Err(CodecError::malformed("BREP projection has no typed arena")),
                };
            }
        }
    }
    Err(CodecError::malformed("BREP projection has no typed arena"))
}

fn select_rows<T>(
    ctx: &DecodeContext<'_>,
    rows: Vec<T>,
    items: &[Value],
    reachable: &[String],
) -> Result<Vec<T>, CodecError> {
    if rows.len() != items.len() {
        return Err(CodecError::malformed(
            "BREP projection changed the typed row count",
        ));
    }
    let mut retained = Vec::new();
    for (row, item) in rows.into_iter().zip(items) {
        ctx.charge_work(1, "select F3D retained BREP rows")?;
        let selected = match entity_id(ctx, item, "select F3D retained BREP rows")? {
            Some(id) => contains(ctx, reachable, id, "select F3D retained BREP rows")?,
            None => true,
        };
        if selected {
            ctx.charge_work(
                u64_from_index(std::mem::size_of::<T>()),
                "move F3D retained BREP row",
            )?;
            ctx.push_retained_vec(&mut retained, row, "collect F3D retained BREP rows")?;
        }
    }
    Ok(retained)
}

fn target_selected(
    ctx: &DecodeContext<'_>,
    target: &AttributeTarget,
    reachable: &[String],
) -> Result<bool, CodecError> {
    let id = match target {
        AttributeTarget::Document => return Ok(true),
        AttributeTarget::Body(id) => id.as_str(),
        AttributeTarget::Face(id) => id.as_str(),
        AttributeTarget::Shell(id) => id.as_str(),
        AttributeTarget::Loop(id) => id.as_str(),
        AttributeTarget::Coedge(id) => id.as_str(),
        AttributeTarget::Edge(id) => id.as_str(),
        AttributeTarget::Vertex(id) => id.as_str(),
    };
    contains(ctx, reachable, id, "select F3D retained attribute target")
}

fn select_links<T>(
    ctx: &DecodeContext<'_>,
    rows: Vec<T>,
    reachable: &[String],
    target: impl Fn(&T) -> &AttributeTarget,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let mut retained = Vec::new();
    for row in rows {
        ctx.charge_work(1, operation)?;
        if target_selected(ctx, target(&row), reachable)? {
            ctx.charge_work(u64_from_index(std::mem::size_of::<T>()), operation)?;
            ctx.push_retained_vec(&mut retained, row, operation)?;
        }
    }
    Ok(retained)
}

impl Brep {
    /// Retain the connected graph rooted at the selected native body selectors.
    pub(crate) fn retain_body_keys(
        &mut self,
        ctx: &DecodeContext<'_>,
        selected_keys: &HashSet<u64>,
    ) -> Result<(), CodecError> {
        let projected = projection(ctx, self, false, "project F3D retained BREP value")?;
        let reachable = ctx.with_scoped_storage("index F3D retained BREP graph", || {
            let mut owned = Vec::new();
            collect_owned_ids(ctx, &projected, &mut owned)?;
            let mut native_bodies = Vec::new();
            for native in &self.asm.body_native_keys {
                ctx.charge_work(1, "walk F3D native BREP bodies")?;
                let body =
                    ctx.copy_retained_text(native.body.as_str(), "copy F3D native BREP body")?;
                insert_id(
                    ctx,
                    &mut native_bodies,
                    body,
                    "index F3D native BREP bodies",
                )?;
            }
            let mut reachable = Vec::new();
            for body in self.body_selectors_for(ctx, selected_keys)?.into_keys() {
                ctx.charge_work(1, "walk F3D selected BREP roots")?;
                insert_id(
                    ctx,
                    &mut reachable,
                    body.into_string(),
                    "collect F3D selected BREP roots",
                )?;
            }
            for body in &self.asm.bodies {
                ctx.charge_work(1, "walk F3D neutral BREP roots")?;
                if !contains(
                    ctx,
                    &native_bodies,
                    body.id.as_str(),
                    "find F3D native BREP root",
                )? {
                    let id =
                        ctx.copy_retained_text(body.id.as_str(), "copy F3D neutral BREP root")?;
                    insert_id(ctx, &mut reachable, id, "collect F3D neutral BREP roots")?;
                }
            }
            let adjacency = adjacency(ctx, &projected, &owned)?;
            let mut pending = Vec::new();
            for id in &reachable {
                ctx.charge_work(1, "walk F3D BREP pending roots")?;
                ctx.push_retained_vec(
                    &mut pending,
                    ctx.copy_retained_text(id.as_ref(), "copy F3D BREP pending root")?,
                    "collect F3D BREP pending roots",
                )?;
            }
            while let Some(id) = pending.pop() {
                ctx.charge_work(1, "walk F3D BREP pending IDs")?;
                let adjacent_ids = position(ctx, &adjacency, &id, |row| row.source.as_str())?
                    .ok()
                    .map(|index| &adjacency[index].targets);
                ctx.charge_work(0, "find F3D BREP adjacent IDs")?;
                for adjacent in adjacent_ids.into_iter().flatten() {
                    ctx.charge_work(1, "walk F3D BREP adjacent IDs")?;
                    let adjacent = adjacent.as_ref();
                    if !contains(ctx, &reachable, adjacent, "find F3D reachable BREP ID")? {
                        insert_id(
                            ctx,
                            &mut reachable,
                            ctx.copy_retained_text(adjacent, "copy F3D reachable BREP ID")?,
                            "collect F3D reachable BREP IDs",
                        )?;
                        ctx.push_retained_vec(
                            &mut pending,
                            ctx.copy_retained_text(adjacent, "copy F3D pending BREP ID")?,
                            "collect F3D pending BREP IDs",
                        )?;
                    }
                }
            }
            Ok::<_, CodecError>(reachable)
        })?;
        macro_rules! select {
            ($($field:ident),*) => {$(
                let items = arena(ctx, &projected, stringify!($field))?;
                self.asm.$field = select_rows(ctx, std::mem::take(&mut self.asm.$field), items, &reachable.0)?;
            )*};
        }
        select!(
            bodies,
            regions,
            shells,
            faces,
            loops,
            coedges,
            edges,
            vertices,
            points,
            surfaces,
            curves,
            pcurves,
            procedural_surfaces,
            procedural_curves,
            edge_continuities,
            edge_ownerships,
            vertex_ownerships,
            face_sidedness,
            face_native_keys,
            tolerant_coedge_parameters,
            tolerant_edge_tails,
            tolerant_vertex_tails,
            mesh_surface_sentinels,
            transform_hints,
            body_native_keys,
            wire_topologies,
            attributes,
            unknowns
        );
        let mut annotations = Vec::new();
        for row in std::mem::take(&mut self.asm.annotation_records) {
            ctx.charge_work(1, "walk F3D retained annotations")?;
            if contains(
                ctx,
                &reachable.0,
                &row.id,
                "select F3D retained annotations",
            )? {
                ctx.charge_work(
                    u64_from_index(std::mem::size_of_val(&row)),
                    "move F3D retained annotation",
                )?;
                ctx.push_retained_vec(&mut annotations, row, "collect F3D retained annotations")?;
            }
        }
        self.asm.annotation_records = annotations;
        self.sketch_curve_links = select_links(
            ctx,
            std::mem::take(&mut self.sketch_curve_links),
            &reachable.0,
            |row| &row.target,
            "collect F3D retained sketch links",
        )?;
        self.persistent_design_links = select_links(
            ctx,
            std::mem::take(&mut self.persistent_design_links),
            &reachable.0,
            |row| &row.target,
            "collect F3D retained design links",
        )?;
        self.persistent_subentity_tags = select_links(
            ctx,
            std::mem::take(&mut self.persistent_subentity_tags),
            &reachable.0,
            |row| &row.target,
            "collect F3D retained subentity tags",
        )?;
        self.creation_timestamps = select_links(
            ctx,
            std::mem::take(&mut self.creation_timestamps),
            &reachable.0,
            |row| &row.target,
            "collect F3D retained timestamps",
        )?;
        Ok(())
    }

    /// Qualify owned identities and their exact text references in the typed graph.
    pub(crate) fn qualify_ids(
        &mut self,
        ctx: &DecodeContext<'_>,
        format: IdFormat,
        namespace: &str,
    ) -> Result<(), CodecError> {
        let projected = projection(ctx, self, true, "project F3D qualified BREP value")?;
        let replacements = ctx.with_scoped_storage("index F3D BREP replacements", || {
            let mut owned = Vec::new();
            collect_owned_ids(ctx, &projected, &mut owned)?;
            let prefix =
                ctx.format_retained(format_args!("{format}:"), "retain F3D BREP scheme prefix")?;
            let mut replacements = Vec::new();
            for id in owned {
                ctx.charge_work(1, "walk F3D BREP replacements")?;
                let source = id.as_str();
                let suffix = match source.get(..prefix.len()) {
                    Some(start)
                        if compare(ctx, start, &prefix, "match F3D BREP scheme prefix")?
                            .is_eq() =>
                    {
                        source.get(prefix.len()..).unwrap_or(source)
                    }
                    _ => source,
                };
                let replacement = ctx.format_retained(
                    format_args!("{format}:brep/{namespace}/{suffix}"),
                    "retain F3D qualified BREP ID",
                )?;
                ctx.charge_work(1, "move F3D BREP replacement")?;
                ctx.push_retained_vec(
                    &mut replacements,
                    (id, replacement),
                    "index F3D BREP replacements",
                )?;
            }
            Ok::<_, CodecError>(replacements)
        })?;
        let mut map =
            IdentityMap::new(ctx, "rewrite F3D qualified BREP fields", |source: &str| {
                ctx.charge_work(0, "find F3D BREP replacement")?;
                let replacement = position(ctx, &replacements.0, source, |row| row.0.as_str())?
                    .ok()
                    .map(|index| &replacements.0[index].1);
                ctx.charge_work(0, "find F3D BREP replacement")?;
                ctx.copy_retained_text(
                    replacement.map_or(source, String::as_str),
                    "copy F3D BREP remapped ID",
                )
            })?
            .with_text_replacements(
                replacements
                    .0
                    .iter()
                    .map(|(source, target)| (source, target)),
            )?;
        let source = std::mem::take(self);
        let rewritten = source.rewrite_identities(ctx, &mut map);
        map.finish(ctx)?;
        *self = rewritten?;
        Ok(())
    }
}

impl RewriteIdentities for Brep {
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk F3D BREP typed references")?;
        ctx.charge_work(1, "walk F3D BREP typed references")?;
        self.asm.visit_identity_references(ctx, visitor)?;
        self.sketch_curve_links
            .visit_identity_references(ctx, visitor)?;
        self.persistent_design_links
            .visit_identity_references(ctx, visitor)?;
        self.persistent_subentity_tags
            .visit_identity_references(ctx, visitor)?;
        self.creation_timestamps
            .visit_identity_references(ctx, visitor)
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        mut self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("rewrite F3D BREP typed fields")?;
        ctx.charge_work(1, "rewrite F3D BREP typed fields")?;
        self.asm = self.asm.rewrite_identities(ctx, map)?;
        self.sketch_curve_links = self.sketch_curve_links.rewrite_identities(ctx, map)?;
        self.persistent_design_links = self.persistent_design_links.rewrite_identities(ctx, map)?;
        self.persistent_subentity_tags = self
            .persistent_subentity_tags
            .rewrite_identities(ctx, map)?;
        self.creation_timestamps = self.creation_timestamps.rewrite_identities(ctx, map)?;
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::{collect_owned_ids, Brep};
    use cadmpeg_asm::brep::AsmBrep;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::ids::{BodyId, PointId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::{Body, BodyKind, Point};
    use std::collections::HashSet;

    fn graph() -> Brep {
        Brep {
            asm: AsmBrep {
                bodies: vec![Body {
                    id: BodyId::mint("f3d:brep:entity#1").unwrap(),
                    kind: BodyKind::default(),
                    regions: vec![],
                    transform: None,
                    name: Some("f3d:brep:entity#1".into()),
                    color: None,
                    visible: None,
                }],
                points: vec![Point::new(
                    PointId::mint("f3d:brep:point#2").unwrap(),
                    FinitePoint3::new(Point3 {
                        x: f64::MAX,
                        y: f64::MIN_POSITIVE,
                        z: -0.0,
                    })
                    .unwrap(),
                    None,
                )],
                ..AsmBrep::default()
            },
            ..Brep::default()
        }
    }

    #[test]
    fn typed_brep_qualification_preserves_coordinate_bits_and_plain_identity_text() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 65536;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut brep = graph();
        let original = brep.asm.points[0].position();
        brep.qualify_ids(&ctx, crate::ids::ID_FORMAT, "source")
            .unwrap();
        assert_eq!(
            brep.asm.bodies[0].id.as_str(),
            "f3d:brep/source/brep:entity#1"
        );
        assert_eq!(
            brep.asm.bodies[0].name.as_deref(),
            Some("f3d:brep/source/brep:entity#1")
        );
        assert_eq!(
            brep.asm.points[0].id.as_str(),
            "f3d:brep/source/brep:point#2"
        );
        assert_eq!(
            brep.asm.points[0].position().x.to_bits(),
            original.x.to_bits()
        );
        assert_eq!(
            brep.asm.points[0].position().y.to_bits(),
            original.y.to_bits()
        );
        assert_eq!(
            brep.asm.points[0].position().z.to_bits(),
            original.z.to_bits()
        );
        let storage = ctx
            .reserve_scoped(65536, "BREP graph scratch released")
            .unwrap();
        drop(storage);
        ctx.finish_session().unwrap();
    }

    #[test]
    fn typed_brep_retention_moves_existing_coordinate_bits() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let mut brep = graph();
        brep.asm.bodies[0].name = Some("f3d:brep:point#2".into());
        let original = brep.asm.points[0].position();
        brep.retain_body_keys(&ctx, &HashSet::new()).unwrap();
        assert_eq!(brep.asm.points.len(), 1);
        assert_eq!(
            brep.asm.points[0].position().x.to_bits(),
            original.x.to_bits()
        );
        assert_eq!(
            brep.asm.points[0].position().y.to_bits(),
            original.y.to_bits()
        );
        assert_eq!(
            brep.asm.points[0].position().z.to_bits(),
            original.z.to_bits()
        );
        ctx.finish_session().unwrap();
    }

    #[test]
    fn typed_brep_graph_paths_preserve_every_refusal_dimension() {
        for qualification in [false, true] {
            for dimension in [
                ResourceDimension::RetainedBytes,
                ResourceDimension::MaterializedBytes,
                ResourceDimension::CollectionItems,
                ResourceDimension::WorkUnits,
                ResourceDimension::RecursionDepth,
            ] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                match dimension {
                    ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
                    ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = 0;
                    }
                    ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                    ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                    ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
                    _ => panic!("BREP dimensions"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let mut brep = graph();
                let error = if qualification {
                    brep.qualify_ids(&ctx, crate::ids::ID_FORMAT, "source")
                } else {
                    brep.retain_body_keys(&ctx, &HashSet::new())
                }
                .unwrap_err();
                let CodecError::ResourceLimit(first) = error else {
                    panic!("BREP graph must refuse");
                };
                assert_eq!(first.dimension, dimension);
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == first)
                );
            }
        }
    }

    #[test]
    fn brep_owned_id_scan_admits_record_keys_before_searching() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let value = serde_value::to_value(serde_json::json!({"id":"f3d:brep:point#2"})).unwrap();
        let CodecError::ResourceLimit(first) =
            collect_owned_ids(&ctx, &value, &mut Vec::new()).unwrap_err()
        else {
            panic!("key search must refuse");
        };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.operation, "walk F3D BREP owned IDs");
        assert_eq!(first.additional, 1);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == first)
        );
    }
}
