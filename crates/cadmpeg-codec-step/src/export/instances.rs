//! The product graph the writer emits: the document's product definitions
//! and occurrences, with each instance that shows its own bodies or nested
//! placements placed as an occurrence of a product definition of its own
//! (CADIR v11 §9.4).

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::design::selection::{OccurrencePath, StepRef};
use cadmpeg_ir::ids::{BodyId, OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{Occurrence, ProductDefinition, Storage};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

/// The product definitions and occurrences to emit.
pub(super) struct ProductGraph<'a> {
    /// The document's definitions, then the definitions of split instances.
    pub(super) definitions: Vec<Cow<'a, ProductDefinition>>,
    /// The document's occurrences, then the occurrences the split instances'
    /// definitions own.
    pub(super) occurrences: Vec<Cow<'a, Occurrence>>,
    /// For each definition of a split instance, the document definition whose
    /// product it defines.
    pub(super) shared_products: HashMap<ProductDefinitionId, ProductDefinitionId>,
    /// The paths of the split instances, from their owning definitions.
    pub(super) split: Vec<String>,
    /// The instance paths that name no occurrence of the document.
    pub(super) unresolved: Vec<String>,
    /// The document definitions that every instance of which was split, so
    /// that nothing places them, each with its bodies that no emitted
    /// definition shows. Neither the definitions nor the occurrences they own
    /// are in the graph.
    pub(super) omitted: Vec<(ProductDefinitionId, Vec<BodyId>)>,
}

/// What the definition that owns a path states for the instance at it.
#[derive(Default)]
struct Instance {
    bodies: Option<Vec<BodyId>>,
    placement: Option<Transform>,
    nested: BTreeMap<OccurrenceId, Instance>,
}

impl Instance {
    fn at(&mut self, path: &[OccurrenceId]) -> &mut Self {
        path.iter().fold(self, |instance, occurrence| {
            instance.nested.entry(occurrence.clone()).or_default()
        })
    }
}

/// Builds the graph. Each definition's instances are split after the
/// definitions it places, so a split instance copies its prototype's own
/// splits.
pub(super) fn lower(ir: &CadIr) -> ProductGraph<'_> {
    let graph = ProductGraph {
        definitions: ir
            .model
            .product_definitions
            .iter()
            .map(Cow::Borrowed)
            .collect(),
        occurrences: ir.model.occurrences.iter().map(Cow::Borrowed).collect(),
        shared_products: HashMap::new(),
        split: Vec::new(),
        unresolved: Vec::new(),
        omitted: Vec::new(),
    };
    let mut lowering = Lowering {
        definition_index: index(&graph.definitions, |definition| &definition.id),
        occurrence_index: index(&graph.occurrences, |occurrence| &occurrence.id),
        origins: ir
            .model
            .occurrences
            .iter()
            .map(|occurrence| occurrence.id.clone())
            .collect(),
        graph,
    };
    let document = Document::new(ir);
    for definition in placement_order(ir) {
        let mut root = Instance::default();
        for instance in &definition.instance_bodies {
            match document.resolve(&definition.id, &instance.occurrence) {
                Some(path) => root.at(&path).bodies = Some(instance.bodies.clone()),
                None => lowering
                    .graph
                    .unresolved
                    .push(display(&definition.id, &instance.occurrence)),
            }
        }
        for occurrence in definition
            .occurrences
            .iter()
            .filter_map(|id| document.occurrences.get(id).copied())
        {
            for nested in &occurrence.overrides {
                let Some(transform) = nested.transform else {
                    continue;
                };
                match document.resolve(&occurrence.prototype, &nested.path) {
                    Some(path) => {
                        root.at(std::slice::from_ref(&occurrence.id))
                            .at(&path)
                            .placement = Some(transform);
                    }
                    None => lowering
                        .graph
                        .unresolved
                        .push(display(&definition.id, &nested.path)),
                }
            }
        }
        let Some(&owner) = lowering.definition_index.get(&definition.id) else {
            continue;
        };
        for (occurrence, instance) in root.nested {
            lowering.apply(owner, &occurrence, instance, definition.id.as_str());
        }
    }
    let mut graph = lowering.graph;
    prune(ir, &mut graph);
    graph
}

/// Removes the document definitions that only split instances placed, and
/// the occurrences they own, so that no definition the source places once
/// is written beside its split instance as a root of its own.
fn prune(ir: &CadIr, graph: &mut ProductGraph<'_>) {
    let placed = ir
        .model
        .occurrences
        .iter()
        .map(|occurrence| &occurrence.prototype)
        .collect::<HashSet<_>>();
    let mut owners = HashMap::<&OccurrenceId, &ProductDefinitionId>::new();
    for definition in &graph.definitions {
        for occurrence in &definition.occurrences {
            owners.entry(occurrence).or_insert(&definition.id);
        }
    }
    let mut dropped = HashSet::<ProductDefinitionId>::new();
    loop {
        let live = graph
            .occurrences
            .iter()
            .filter(|occurrence| {
                owners
                    .get(&occurrence.id)
                    .is_none_or(|owner| !dropped.contains(*owner))
            })
            .map(|occurrence| &occurrence.prototype)
            .collect::<HashSet<_>>();
        let newly = graph
            .definitions
            .iter()
            .map(|definition| &definition.id)
            .filter(|id| placed.contains(id) && !live.contains(id) && !dropped.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        if newly.is_empty() {
            break;
        }
        dropped.extend(newly);
    }
    if dropped.is_empty() {
        return;
    }
    let owned_occurrences = graph
        .definitions
        .iter()
        .filter(|definition| dropped.contains(&definition.id))
        .flat_map(|definition| definition.occurrences.iter().cloned())
        .collect::<HashSet<_>>();
    graph
        .occurrences
        .retain(|occurrence| !owned_occurrences.contains(&occurrence.id));
    let (omitted, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut graph.definitions)
        .into_iter()
        .partition(|definition| dropped.contains(&definition.id));
    let shown = kept
        .iter()
        .flat_map(|definition| definition.bodies.iter())
        .collect::<HashSet<_>>();
    graph.omitted = omitted
        .iter()
        .map(|definition| {
            let bodies = definition
                .bodies
                .iter()
                .filter(|body| !shown.contains(body))
                .cloned()
                .collect();
            (definition.id.clone(), bodies)
        })
        .collect();
    graph.definitions = kept;
}

struct Lowering<'a> {
    graph: ProductGraph<'a>,
    definition_index: HashMap<ProductDefinitionId, usize>,
    occurrence_index: HashMap<OccurrenceId, usize>,
    /// For each graph occurrence, the document occurrence it copies.
    origins: Vec<OccurrenceId>,
}

impl Lowering<'_> {
    /// Applies `instance` to the occurrence of the definition at `owner` that
    /// copies document occurrence `step`.
    fn apply(&mut self, owner: usize, step: &OccurrenceId, instance: Instance, prefix: &str) {
        let path = format!("{prefix}/{}", step.as_str());
        let target = self.graph.definitions[owner]
            .occurrences
            .iter()
            .filter_map(|id| self.occurrence_index.get(id).copied())
            .find(|&index| self.origins[index] == *step);
        let Some(target) = target else {
            self.graph.unresolved.push(path);
            return;
        };
        let variant = if instance.bodies.is_none() && instance.nested.is_empty() {
            None
        } else {
            let prototype = self.graph.occurrences[target].prototype.clone();
            let Some(variant) = self.split(&prototype, instance.bodies) else {
                self.graph.unresolved.push(path);
                return;
            };
            Some(variant)
        };
        if let Some(transform) = instance.placement {
            self.graph.occurrences[target].to_mut().transform = Some(transform);
        }
        let Some(variant) = variant else {
            return;
        };
        let variant_id = self.graph.definitions[variant].id.clone();
        self.graph.occurrences[target].to_mut().prototype = variant_id;
        self.graph.split.push(path.clone());
        for (nested, instance) in instance.nested {
            self.apply(variant, &nested, instance, &path);
        }
    }

    /// Adds a copy of `prototype` that owns copies of its occurrences and
    /// shows `bodies` where they are stated.
    fn split(
        &mut self,
        prototype: &ProductDefinitionId,
        bodies: Option<Vec<BodyId>>,
    ) -> Option<usize> {
        let base = *self.definition_index.get(prototype)?;
        let ordinal = self.graph.shared_products.len() + 1;
        let tail = format!("-instance{ordinal}");
        let mut variant = self.graph.definitions[base].clone().into_owned();
        variant.id = ProductDefinitionId::mint(format!("{}{tail}", prototype.as_str())).ok()?;
        variant.storage = Storage::Embedded;
        variant.instance_bodies.clear();
        if let Some(bodies) = bodies {
            variant.bodies = bodies;
        }
        let mut copies = Vec::with_capacity(variant.occurrences.len());
        for id in &variant.occurrences {
            let source = *self.occurrence_index.get(id)?;
            let mut copy = self.graph.occurrences[source].clone().into_owned();
            copy.id = OccurrenceId::mint(format!("{}{tail}", id.as_str())).ok()?;
            copy.overrides.clear();
            copies.push((copy, self.origins[source].clone()));
        }
        variant.occurrences = copies.iter().map(|(copy, _)| copy.id.clone()).collect();
        for (copy, origin) in copies {
            self.occurrence_index
                .insert(copy.id.clone(), self.graph.occurrences.len());
            self.graph.occurrences.push(Cow::Owned(copy));
            self.origins.push(origin);
        }
        let product = self
            .graph
            .shared_products
            .get(prototype)
            .cloned()
            .unwrap_or_else(|| prototype.clone());
        self.graph
            .shared_products
            .insert(variant.id.clone(), product);
        self.definition_index
            .insert(variant.id.clone(), self.graph.definitions.len());
        self.graph.definitions.push(Cow::Owned(variant));
        Some(self.graph.definitions.len() - 1)
    }
}

fn index<T, K: Clone + Eq + std::hash::Hash>(
    items: &[Cow<'_, T>],
    key: impl Fn(&T) -> &K,
) -> HashMap<K, usize>
where
    T: Clone,
{
    items
        .iter()
        .enumerate()
        .map(|(position, item)| (key(item).clone(), position))
        .collect()
}

/// The document's definitions, each after every definition it places.
fn placement_order(ir: &CadIr) -> Vec<&ProductDefinition> {
    let definitions = ir
        .model
        .product_definitions
        .iter()
        .map(|definition| (&definition.id, definition))
        .collect::<HashMap<_, _>>();
    let prototypes = ir
        .model
        .occurrences
        .iter()
        .map(|occurrence| (&occurrence.id, &occurrence.prototype))
        .collect::<HashMap<_, _>>();
    let mut order = Vec::with_capacity(definitions.len());
    let mut visited = HashSet::new();
    for definition in &ir.model.product_definitions {
        let mut stack = vec![(definition, false)];
        while let Some((definition, placed)) = stack.pop() {
            if placed {
                order.push(definition);
                continue;
            }
            if !visited.insert(&definition.id) {
                continue;
            }
            stack.push((definition, true));
            for occurrence in &definition.occurrences {
                if let Some(child) = prototypes
                    .get(occurrence)
                    .and_then(|prototype| definitions.get(prototype))
                {
                    stack.push((child, false));
                }
            }
        }
    }
    order
}

/// The document's definitions and occurrences by identity.
struct Document<'a> {
    definitions: HashMap<&'a ProductDefinitionId, &'a ProductDefinition>,
    occurrences: HashMap<&'a OccurrenceId, &'a Occurrence>,
}

impl<'a> Document<'a> {
    fn new(ir: &'a CadIr) -> Self {
        Self {
            definitions: ir
                .model
                .product_definitions
                .iter()
                .map(|definition| (&definition.id, definition))
                .collect(),
            occurrences: ir
                .model
                .occurrences
                .iter()
                .map(|occurrence| (&occurrence.id, occurrence))
                .collect(),
        }
    }

    /// The occurrences of a nonempty path from `owner` whose every step
    /// names an occurrence that the previous step's prototype owns.
    fn resolve(
        &self,
        owner: &'a ProductDefinitionId,
        path: &OccurrencePath,
    ) -> Option<Vec<OccurrenceId>> {
        if path.0.is_empty() {
            return None;
        }
        let mut current = owner;
        path.0
            .iter()
            .map(|step| {
                let StepRef::Occurrence(id) = &step.occurrence else {
                    return None;
                };
                if !self.definitions.get(current)?.occurrences.contains(id) {
                    return None;
                }
                current = &self.occurrences.get(id)?.prototype;
                Some(id.clone())
            })
            .collect()
    }
}

fn display(owner: &ProductDefinitionId, path: &OccurrencePath) -> String {
    std::iter::once(owner.as_str().to_owned())
        .chain(path.0.iter().map(|step| match &step.occurrence {
            StepRef::Occurrence(id) => id.as_str().to_owned(),
            StepRef::Native(name) => format!("{:?}", name.value),
        }))
        .collect::<Vec<_>>()
        .join("/")
}
