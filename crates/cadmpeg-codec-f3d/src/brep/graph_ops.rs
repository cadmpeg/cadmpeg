// SPDX-License-Identifier: Apache-2.0
//! Select dependencies and qualify identities in the typed BREP graph.

use super::Brep;
use cadmpeg_asm::ids::IdFormat;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use std::collections::{HashMap, HashSet};

pub(super) mod ordered;

use ordered::{select_links, select_rows};

type BrepAdjacency = HashMap<String, HashSet<String>>;

pub(super) fn insert_brep_adjacency(adjacency: &mut BrepAdjacency, source: &str, target: &str) {
    adjacency
        .entry(source.to_owned())
        .or_default()
        .insert(target.to_owned());
}

fn add_dependencies<T: RewriteIdentities>(
    ctx: &DecodeContext<'_>,
    adjacency: &mut BrepAdjacency,
    owner: &str,
    row: &T,
) -> Result<(), CodecError> {
    row.visit_identity_references(ctx, &mut |target| {
        if owner != target {
            insert_brep_adjacency(adjacency, owner, target);
        }
        Ok(())
    })
}

impl Brep {
    /// Retain the dependencies rooted at the selected native body selectors.
    pub(crate) fn retain_body_keys(
        &mut self,
        ctx: &DecodeContext<'_>,
        selected_keys: &HashSet<u64>,
    ) -> Result<(), CodecError> {
        let selected = self
            .body_selectors_for(selected_keys)?
            .into_keys()
            .collect();
        self.retain_bodies(ctx, selected)
    }

    /// Remove the sketch-region bodies of a blob that stores no body key, when
    /// the blob's body map holds one selector per remaining body, and number
    /// the remaining bodies by their selector position. Returns the number of
    /// removed bodies.
    pub(crate) fn omit_sketch_region_bodies(
        &mut self,
        ctx: &DecodeContext<'_>,
        selector_count: usize,
    ) -> Result<usize, CodecError> {
        if self
            .asm
            .body_native_keys
            .iter()
            .any(|key| key.asm_body_key.is_some())
        {
            return Ok(0);
        }
        let regions = self.sketch_region_bodies();
        if regions.is_empty() {
            return Ok(0);
        }
        let selected = self
            .asm
            .body_native_keys
            .iter()
            .map(|key| key.body.clone())
            .filter(|body| !regions.contains(body))
            .collect::<Vec<_>>();
        if selected.len() != selector_count {
            return Ok(0);
        }
        self.retain_bodies(ctx, selected)?;
        self.asm
            .body_native_keys
            .sort_by_key(|key| key.body_ordinal);
        for (ordinal, key) in self.asm.body_native_keys.iter_mut().enumerate() {
            key.body_ordinal = u32::try_from(ordinal)
                .map_err(|_| CodecError::malformed("F3D BREP body ordinal exceeds u32"))?;
        }
        Ok(regions.len())
    }

    /// Bodies with at least one edge, whose every edge stores a sketch-curve
    /// link, and whose faces store no attribute.
    fn sketch_region_bodies(&self) -> HashSet<BodyId> {
        let mut linked_edges = HashSet::new();
        for link in &self.sketch_curve_links {
            if let AttributeTarget::Edge(edge) = &link.target {
                linked_edges.insert(edge.as_str());
            }
        }
        let mut attributed_faces = HashSet::new();
        for attribute in &self.asm.attributes {
            if let AttributeTarget::Face(face) = &attribute.target.entity {
                attributed_faces.insert(face.as_str());
            }
        }
        let mut loops_by_face = HashMap::<&str, Vec<&str>>::new();
        for row in &self.asm.loops {
            loops_by_face
                .entry(row.face.as_str())
                .or_default()
                .push(row.id.as_str());
        }
        let mut faces_by_shell = HashMap::<&str, Vec<&str>>::new();
        for face in &self.asm.faces {
            faces_by_shell
                .entry(face.shell.as_str())
                .or_default()
                .push(face.id.as_str());
        }
        let mut edges_by_loop = HashMap::<&str, Vec<&str>>::new();
        for coedge in &self.asm.coedges {
            edges_by_loop
                .entry(coedge.owner_loop.as_str())
                .or_default()
                .push(coedge.edge.as_str());
        }
        let shells = self
            .asm
            .shells
            .iter()
            .map(|shell| (shell.region.as_str(), shell))
            .fold(
                HashMap::<&str, Vec<_>>::new(),
                |mut by_region, (region, shell)| {
                    by_region.entry(region).or_default().push(shell);
                    by_region
                },
            );
        let mut regions = HashSet::new();
        for row in &self.asm.regions {
            let shells = shells.get(row.id.as_str()).map_or(&[][..], Vec::as_slice);
            let mut edge_count = 0_usize;
            let is_region = shells.iter().all(|shell| {
                shell.wire_edges().next().is_none()
                    && faces_by_shell
                        .get(shell.id.as_str())
                        .into_iter()
                        .flatten()
                        .all(|face| {
                            !attributed_faces.contains(face)
                                && loops_by_face
                                    .get(face)
                                    .into_iter()
                                    .flatten()
                                    .flat_map(|ring| edges_by_loop.get(ring).into_iter().flatten())
                                    .all(|edge| {
                                        edge_count += 1;
                                        linked_edges.contains(edge)
                                    })
                        })
            });
            if is_region && edge_count != 0 {
                regions.insert(row.body.clone());
            } else {
                regions.remove(&row.body);
            }
        }
        regions
    }

    fn retain_bodies(
        &mut self,
        ctx: &DecodeContext<'_>,
        selected: Vec<BodyId>,
    ) -> Result<(), CodecError> {
        let reachable = {
            let mut native_bodies = HashSet::new();
            for native in &self.asm.body_native_keys {
                let body = native.body.as_str().to_owned();
                native_bodies.insert(body);
            }
            let mut reachable = HashSet::new();
            for body in selected {
                reachable.insert(body.into_string());
            }
            for body in &self.asm.bodies {
                if !native_bodies.contains(body.id.as_str()) {
                    let id = body.id.as_str().to_owned();
                    reachable.insert(id);
                }
            }
            let mut adjacency = HashMap::new();
            macro_rules! dependencies {
                ($($field:ident),*) => {$(
                    for row in &self.asm.$field {
                        add_dependencies(ctx, &mut adjacency, row.id.as_str(), row)?;
                    }
                )*};
            }
            dependencies!(
                bodies, regions, shells, faces, loops, coedges, edges, vertices, points, surfaces,
                curves, pcurves
            );
            for region in &self.asm.regions {
                insert_brep_adjacency(&mut adjacency, region.body.as_str(), region.id.as_str());
            }
            for shell in &self.asm.shells {
                insert_brep_adjacency(&mut adjacency, shell.region.as_str(), shell.id.as_str());
            }
            for face in &self.asm.faces {
                insert_brep_adjacency(&mut adjacency, face.shell.as_str(), face.id.as_str());
            }
            macro_rules! native_dependencies {
                ($($field:ident => $owner:ident),*) => {$(
                    for row in &self.asm.$field {
                        add_dependencies(ctx, &mut adjacency, row.$owner.as_str(), row)?;
                    }
                )*};
            }
            native_dependencies!(edge_continuities => edge, edge_ownerships => edge,
                vertex_ownerships => vertex, face_sidedness => face, face_native_keys => face,
                tolerant_coedge_parameters => coedge, tolerant_edge_tails => edge,
                tolerant_vertex_tails => vertex, mesh_surface_sentinels => surface,
                transform_hints => body, body_native_keys => body, wire_topologies => shell);
            for row in &self.asm.unknowns {
                add_dependencies(ctx, &mut adjacency, row.id().as_str(), row)?;
            }
            // Definitions are attached to carriers even when the carrier has a solved cache.
            // A tuple is an association, not an unowned row to retain indiscriminately.
            for (carrier, definition) in &self.asm.procedural_surfaces {
                add_dependencies(ctx, &mut adjacency, carrier.as_str(), definition)?;
            }
            for (carrier, definition) in &self.asm.procedural_curves {
                add_dependencies(ctx, &mut adjacency, carrier.as_str(), definition)?;
            }
            let mut pending = reachable.iter().cloned().collect::<Vec<_>>();
            while let Some(id) = pending.pop() {
                for adjacent in adjacency.get(&id).into_iter().flatten() {
                    if reachable.insert(adjacent.clone()) {
                        pending.push(adjacent.clone());
                    }
                }
            }
            reachable
        };
        macro_rules! select {
            ($($field:ident => $owner:ident),*) => {$(
                self.asm.$field = select_rows(std::mem::take(&mut self.asm.$field), &reachable, |row| row.$owner.as_str());
            )*};
        }
        select!(bodies => id, regions => id, shells => id, faces => id,
            loops => id, coedges => id, edges => id, vertices => id,
            points => id, surfaces => id, curves => id, pcurves => id,
            edge_continuities => edge, edge_ownerships => edge,
            vertex_ownerships => vertex, face_sidedness => face,
            face_native_keys => face, tolerant_coedge_parameters => coedge,
            tolerant_edge_tails => edge, tolerant_vertex_tails => vertex,
            mesh_surface_sentinels => surface, transform_hints => body,
            body_native_keys => body, wire_topologies => shell,
            unbounded_faces => face);
        self.asm.unknowns =
            select_rows(std::mem::take(&mut self.asm.unknowns), &reachable, |row| {
                row.id().as_str()
            });
        self.asm.procedural_surfaces = select_rows(
            std::mem::take(&mut self.asm.procedural_surfaces),
            &reachable,
            |row| row.0.as_str(),
        );
        self.asm.procedural_curves = select_rows(
            std::mem::take(&mut self.asm.procedural_curves),
            &reachable,
            |row| row.0.as_str(),
        );
        self.asm.attributes = select_links(
            std::mem::take(&mut self.asm.attributes),
            &reachable,
            |row| &row.target.entity,
        );
        let mut annotations = Vec::new();
        for row in std::mem::take(&mut self.asm.annotation_records) {
            if reachable.contains(&row.id) {
                annotations.push(row);
            }
        }
        self.asm.annotation_records = annotations;
        self.sketch_curve_links = select_links(
            std::mem::take(&mut self.sketch_curve_links),
            &reachable,
            |row| &row.target,
        );
        self.persistent_design_links = select_links(
            std::mem::take(&mut self.persistent_design_links),
            &reachable,
            |row| &row.target,
        );
        self.persistent_subentity_tags = select_links(
            std::mem::take(&mut self.persistent_subentity_tags),
            &reachable,
            |row| &row.target,
        );
        self.creation_timestamps = select_links(
            std::mem::take(&mut self.creation_timestamps),
            &reachable,
            |row| &row.target,
        );
        Ok(())
    }

    /// Qualify identity fields while preserving ordinary text and numeric payloads.
    pub(crate) fn qualify_ids(
        &mut self,
        ctx: &DecodeContext<'_>,
        format: IdFormat,
        namespace: &str,
    ) -> Result<(), CodecError> {
        let prefix = format!("{format}:");
        let mut map = IdentityMap::new(|source: &str| {
            let suffix = source.strip_prefix(prefix.as_str()).unwrap_or(source);
            Ok(format!("{format}:brep/{namespace}/{suffix}"))
        });
        let source = std::mem::take(self);
        let rewritten = source.rewrite_identities(ctx, &mut map);
        map.finish()?;
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
    fn rewrite_identities(
        mut self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_>,
    ) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("rewrite F3D BREP typed fields")?;

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
    use super::Brep;
    use cadmpeg_asm::brep::AsmBrep;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::features::geometry::FinitePoint3;
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
                    transform: None,
                    name: Some("f3d:brep:entity#1".into()),
                    color: None,
                    visible: None,
                    native_ref: None,
                    unstated: None,
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
        let policy = DecodePolicy::service();
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
            Some("f3d:brep:entity#1")
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
        ctx.finish_session().unwrap();
    }

    #[test]
    fn typed_brep_graph_paths_preserve_recursion_depth_refusal() {
        for qualification in [false, true] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_recursion_depth = 0;
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
            assert_eq!(first.dimension, ResourceDimension::RecursionDepth);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == first)
            );
        }
    }

    #[test]
    fn carrier_selection_keeps_its_procedure_and_support_surface() {
        use cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
        use cadmpeg_ir::geometry::{ProceduralSurface, Surface};
        let mut geometry = cadmpeg_ir::examples::unit_cube().unwrap().geometry;
        let carrier = geometry.faces[0].surface.clone();
        let support = cadmpeg_ir::ids::SurfaceId::mint("f3d:brep:surface#support").unwrap();
        geometry.surfaces.push(Surface {
            id: support.clone(),
            geometry: geometry.surfaces[0].geometry.clone(),
            source_object: None,
            native_ref: None,
        });
        let definition = ProceduralSurface::new(
            "f3d:brep:procedure#replica".try_into().unwrap(),
            ProceduralSurfaceDefinition::Replica {
                source: support.clone(),
                transform: cadmpeg_ir::transform::Transform::identity(),
            },
            None,
        );
        let mut brep = Brep {
            asm: AsmBrep {
                bodies: geometry.bodies,
                regions: geometry.regions,
                shells: geometry.shells,
                faces: geometry.faces,
                loops: geometry.loops,
                coedges: geometry.coedges,
                edges: geometry.edges,
                vertices: geometry.vertices,
                points: geometry.points,
                surfaces: geometry.surfaces,
                curves: geometry.curves,
                pcurves: geometry.pcurves,
                procedural_surfaces: vec![(carrier, definition)],
                ..AsmBrep::default()
            },
            ..Brep::default()
        };
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        brep.retain_body_keys(&ctx, &HashSet::new()).unwrap();
        assert_eq!(brep.asm.procedural_surfaces.len(), 1);
        assert!(brep
            .asm
            .surfaces
            .iter()
            .any(|surface| surface.id == support));
        ctx.finish_session().unwrap();
    }
}
