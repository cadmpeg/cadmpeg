// SPDX-License-Identifier: Apache-2.0
//! Model-space placement of application meshes and their link uses.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::{u64_from_index, DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::products::{Occurrence, OccurrenceParent, PrototypeReference};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::tessellation::{Tessellation, TessellationId};
use cadmpeg_ir::transform::Transform;

use crate::loss::FreecadLossCode;
use crate::native::{ProductNode, ProductNodeRecord, PropertyRecord};

const MAX_MESH_PLACEMENT_WORK: usize = 65_536;

pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    properties: &[PropertyRecord],
    records: &[ProductNodeRecord],
    losses: &mut Vec<LossNote>,
    admitted_entities: &mut u64,
) -> Result<(), CodecError> {
    let mesh_properties = properties
        .iter()
        .filter(|property| property.type_name == "Mesh::PropertyMeshKernel")
        .map(|property| (format!("{}:mesh", property.id), property))
        .collect::<HashMap<_, _>>();
    let mut meshes = HashMap::<&str, Vec<&Tessellation>>::new();
    for mesh in &ir.model.tessellations {
        if let Some(property) = mesh_properties.get(mesh.id.as_str()) {
            meshes.entry(&property.owner).or_default().push(mesh);
        }
    }
    if meshes.is_empty() {
        return Ok(());
    }
    let definitions: HashMap<&str, &str> = ir
        .model
        .product_definitions
        .iter()
        .filter_map(|definition| {
            definition
                .native_ref
                .as_deref()
                .map(|object| (definition.id.as_str(), object))
        })
        .collect();
    let mut own_occurrences = HashMap::new();
    let mut children = HashMap::<&str, Vec<&Occurrence>>::new();
    for occurrence in &ir.model.occurrences {
        if let OccurrenceParent::Occurrence { occurrence: parent } = &occurrence.parent {
            children
                .entry(parent.as_str())
                .or_default()
                .push(occurrence);
        }
        if let (Some(object), PrototypeReference::Local { definition }) =
            (occurrence.native_ref.as_deref(), &occurrence.prototype)
        {
            if definitions.get(definition.as_str()).copied() == Some(object) {
                own_occurrences.insert(object, occurrence);
            }
        }
    }
    let mut owners = HashMap::<&str, Vec<&str>>::new();
    for record in records {
        for member in record.members() {
            owners.entry(member).or_default().push(&record.object);
        }
        if record.external_document().is_none() {
            if let Some(prototype) = record.prototype() {
                owners.entry(prototype).or_default().push(&record.object);
            }
        }
    }
    let mut reachable = meshes.keys().copied().collect::<HashSet<_>>();
    let mut pending = reachable.iter().copied().collect::<Vec<_>>();
    while let Some(object) = pending.pop() {
        for owner in owners.get(object).into_iter().flatten() {
            if reachable.insert(owner) {
                pending.push(owner);
            }
        }
    }
    for children in children.values_mut() {
        children.retain(|child| match &child.prototype {
            PrototypeReference::Local { definition } => definitions
                .get(definition.as_str())
                .is_some_and(|object| reachable.contains(object)),
            _ => false,
        });
    }
    let placements = properties
        .iter()
        .filter(|property| property.name == "Placement")
        .map(|property| (property.owner.as_str(), property))
        .collect::<HashMap<_, _>>();
    let allowance = records
        .len()
        .checked_add(ir.model.occurrences.len())
        .and_then(|count| count.checked_mul(64))
        .ok_or_else(|| CodecError::malformed("mesh placement work allowance overflows"))?
        .max(MAX_MESH_PLACEMENT_WORK);
    let scene = Scene {
        ctx,
        meshes,
        records: records
            .iter()
            .map(|record| (record.object.as_str(), record))
            .collect(),
        occurrences: &ir.model.occurrences,
        definitions,
        own_occurrences,
        children,
        reachable,
        work: WorkBudget::new(allowance),
    };
    let occurrence_index = ir
        .model
        .occurrences
        .iter()
        .map(|occurrence| (occurrence.id.as_str(), occurrence))
        .collect::<HashMap<_, _>>();
    let mut world = HashMap::new();
    let mut output = Vec::new();
    for mesh in &ir.model.tessellations {
        let Some(property) = mesh_properties.get(mesh.id.as_str()) else {
            output.push(mesh.clone());
            continue;
        };
        let own_occurrence = scene.own_occurrences.get(property.owner.as_str()).copied();
        let transform = if let Some(occurrence) = own_occurrence {
            world_transform(
                ctx,
                occurrence,
                &occurrence_index,
                &mut world,
                &mut HashSet::new(),
            )?
        } else {
            let placement = placements.get(property.owner.as_str()).copied();
            placement
                .map(crate::placement::placement_matrix)
                .transpose()?
                .flatten()
                .map(crate::native::frame::FiniteFrame::transform)
                .unwrap_or_default()
        };
        output.push(placed_mesh(mesh, transform, None)?);
    }
    let mut population = u64_from_index(ir.model.entity_count());
    for occurrence in scene.occurrences {
        let Some(record) = occurrence
            .native_ref
            .as_deref()
            .and_then(|object| scene.records.get(object))
            .filter(|record| matches!(record.node, ProductNode::Occurrence(_)))
        else {
            continue;
        };
        let Some(prototype) = scene.prototype(occurrence) else {
            continue;
        };
        if !scene.reachable.contains(prototype) {
            continue;
        }
        let transform = world_transform(
            ctx,
            occurrence,
            &occurrence_index,
            &mut world,
            &mut HashSet::new(),
        )?;
        let first_mesh = output.len();
        let mut sink = Sink {
            output: &mut output,
            losses,
            population: &mut population,
            admitted_entities,
            path: vec![occurrence.id.as_str().to_owned()],
        };
        if record.linked_subelements().is_empty() {
            scene.expand(prototype, transform, &mut Vec::new(), &mut sink)?;
        } else if scene.reachable.contains(prototype) {
            sink.loss(ctx, prototype)?;
        }
        if scene.work.exhausted() {
            sink.output.truncate(first_mesh);
            let message = format!("mesh occurrence {} and subsequent uses are withheld because placement expansion reached its local work bound", occurrence.id);
            ctx.retain_bytes(
                u64_from_index(message.len()),
                "retain FCStd mesh placement loss",
            )?;
            sink.losses
                .push(FreecadLossCode::MeshPlacementBounded.note(message));
            break;
        }
    }
    ir.model.tessellations = output;
    Ok(())
}

struct Scene<'a, 'ctx> {
    ctx: &'a DecodeContext<'ctx>,
    meshes: HashMap<&'a str, Vec<&'a Tessellation>>,
    records: HashMap<&'a str, &'a ProductNodeRecord>,
    occurrences: &'a [Occurrence],
    definitions: HashMap<&'a str, &'a str>,
    own_occurrences: HashMap<&'a str, &'a Occurrence>,
    children: HashMap<&'a str, Vec<&'a Occurrence>>,
    reachable: HashSet<&'a str>,
    work: WorkBudget,
}

struct Sink<'a> {
    output: &'a mut Vec<Tessellation>,
    losses: &'a mut Vec<LossNote>,
    population: &'a mut u64,
    admitted_entities: &'a mut u64,
    path: Vec<String>,
}

impl Sink<'_> {
    fn loss(&mut self, ctx: &DecodeContext<'_>, object: &str) -> Result<(), CodecError> {
        let message = format!(
            "mesh use {} through {object} has unsupported prototype or subelement placement",
            self.path.join("/")
        );
        ctx.retain_bytes(
            u64_from_index(message.len()),
            "retain FCStd mesh placement loss",
        )?;
        self.losses
            .push(FreecadLossCode::MeshPlacementNotTransferred.note(message));
        Ok(())
    }
}

impl Scene<'_, '_> {
    fn prototype<'a>(&'a self, occurrence: &Occurrence) -> Option<&'a str> {
        match &occurrence.prototype {
            PrototypeReference::Local { definition } => {
                self.definitions.get(definition.as_str()).copied()
            }
            _ => None,
        }
    }

    fn expand(
        &self,
        object: &str,
        transform: Transform,
        stack: &mut Vec<String>,
        sink: &mut Sink<'_>,
    ) -> Result<(), CodecError> {
        if !self.reachable.contains(object) || !self.work.charge() {
            return Ok(());
        }
        let _depth = self.ctx.enter_nested("expand FCStd mesh placement")?;
        if stack.iter().any(|entry| entry == object) {
            return Err(CodecError::malformed("mesh prototype cycle"));
        }
        if let Some(meshes) = self.meshes.get(object) {
            for mesh in meshes {
                *sink.population = sink
                    .population
                    .checked_add(1)
                    .ok_or_else(|| CodecError::malformed("FCStd mesh population overflows"))?;
                self.ctx.admit_entities(
                    *sink.population,
                    sink.admitted_entities,
                    "admit FCStd placed meshes",
                )?;
                let vertex_bytes = u64_from_index(mesh.vertex_count()).checked_mul(24);
                let triangle_bytes = u64_from_index(mesh.triangle_count()).checked_mul(12);
                let bytes = vertex_bytes
                    .zip(triangle_bytes)
                    .and_then(|(vertices, triangles)| vertices.checked_add(triangles))
                    .ok_or_else(|| CodecError::malformed("FCStd placed mesh extent overflows"))?;
                self.ctx.retain_bytes(bytes, "retain FCStd placed mesh")?;
                sink.output
                    .push(placed_mesh(mesh, transform, Some(&sink.path))?);
            }
        }
        let Some(record) = self.records.get(object) else {
            return Ok(());
        };
        if !matches!(
            record.node,
            ProductNode::Part(_) | ProductNode::LinkGroup { .. }
        ) {
            sink.loss(self.ctx, object)?;
            return Ok(());
        }
        stack.push(object.to_owned());
        let children = self
            .own_occurrences
            .get(object)
            .and_then(|parent| self.children.get(parent.id.as_str()))
            .map_or(&[][..], Vec::as_slice);
        for child in children {
            let Some(prototype) = self.prototype(child) else {
                continue;
            };
            sink.path.push(child.id.as_str().to_owned());
            let subelements = child
                .native_ref
                .as_deref()
                .and_then(|object| self.records.get(object))
                .is_some_and(|record| !record.linked_subelements().is_empty());
            if subelements {
                if !self.work.charge() {
                    sink.path.pop();
                    break;
                }
                if self.reachable.contains(prototype) {
                    sink.loss(self.ctx, prototype)?;
                }
                sink.path.pop();
                continue;
            }
            self.expand(
                prototype,
                transform
                    .compose(child.effective_transform().map_err(CodecError::malformed)?)
                    .map_err(CodecError::malformed)?,
                stack,
                sink,
            )?;
            sink.path.pop();
            if self.work.exhausted() {
                break;
            }
        }
        stack.pop();
        Ok(())
    }
}

fn world_transform(
    ctx: &DecodeContext<'_>,
    occurrence: &Occurrence,
    index: &HashMap<&str, &Occurrence>,
    cache: &mut HashMap<String, Transform>,
    active: &mut HashSet<String>,
) -> Result<Transform, CodecError> {
    let _depth = ctx.enter_nested("compose FCStd mesh placement")?;
    if let Some(transform) = cache.get(occurrence.id.as_str()) {
        return Ok(*transform);
    }
    if !active.insert(occurrence.id.as_str().to_owned()) {
        return Err(CodecError::malformed("mesh occurrence parent cycle"));
    }
    let parent = match &occurrence.parent {
        OccurrenceParent::Root {} => Transform::identity(),
        OccurrenceParent::Occurrence { occurrence: parent } => world_transform(
            ctx,
            index
                .get(parent.as_str())
                .ok_or_else(|| CodecError::malformed("mesh occurrence parent is absent"))?,
            index,
            cache,
            active,
        )?,
    };
    let transform = parent
        .compose(
            occurrence
                .effective_transform()
                .map_err(CodecError::malformed)?,
        )
        .map_err(CodecError::malformed)?;
    active.remove(occurrence.id.as_str());
    cache.insert(occurrence.id.as_str().to_owned(), transform);
    Ok(transform)
}

fn placed_mesh(
    mesh: &Tessellation,
    transform: Transform,
    path: Option<&[String]>,
) -> Result<Tessellation, CodecError> {
    let mut output = mesh.clone();
    output
        .edit_vertices(|point| {
            *point = transform
                .apply_point(*point)
                .ok_or_else(|| {
                    cadmpeg_ir::tessellation::TessellationError::EditRefused(
                        "mesh placement produces a non-finite coordinate".to_owned(),
                    )
                })?
                .get();
            Ok(())
        })
        .map_err(CodecError::malformed)?;
    if let Some(path) = path {
        output.id = TessellationId::mint(crate::native::model_id(
            "tessellation",
            mesh.id.as_str(),
            path.join("/"),
        ))
        .map_err(CodecError::malformed)?;
        if let Some(source) = &mut output.source_object {
            source.instance_path = path.to_vec();
        }
    }
    Ok(output)
}
