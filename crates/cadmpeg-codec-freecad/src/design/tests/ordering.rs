// SPDX-License-Identifier: Apache-2.0
//! Design object ordering tests.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write;

use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FeatureId;

use crate::native::{ObjectRecord, PropertyRecord};

type Ordering = Result<(BTreeMap<String, u64>, BTreeSet<String>), String>;

/// Deterministic xorshift sequence.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(bound).unwrap()).unwrap()
    }
}

struct GeneratedObject {
    type_name: &'static str,
    dependencies: BTreeSet<usize>,
    links: Vec<(&'static str, usize)>,
    expressions: Vec<usize>,
    members: BTreeSet<usize>,
}

impl GeneratedObject {
    fn new(type_name: &'static str) -> Self {
        Self {
            type_name,
            dependencies: BTreeSet::new(),
            links: Vec::new(),
            expressions: Vec::new(),
            members: BTreeSet::new(),
        }
    }
}

fn object_name(index: usize) -> String {
    format!("Object{index}")
}

/// Document.xml carrying the declared dependencies, links, expressions and
/// body membership of `objects`.
fn generated_document(objects: &[GeneratedObject]) -> String {
    let mut xml = String::new();
    write!(
        xml,
        r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="{}" Dependencies="1">"#,
        objects.len()
    )
    .unwrap();
    for (index, object) in objects.iter().enumerate() {
        write!(
            xml,
            r#"<ObjectDeps Name="{}" Count="{}">"#,
            object_name(index),
            object.dependencies.len()
        )
        .unwrap();
        for &dependency in &object.dependencies {
            write!(xml, r#"<Dep Name="{}"/>"#, object_name(dependency)).unwrap();
        }
        xml.push_str("</ObjectDeps>");
    }
    for (index, object) in objects.iter().enumerate() {
        write!(
            xml,
            r#"<Object type="{}" name="{}"/>"#,
            object.type_name,
            object_name(index)
        )
        .unwrap();
    }
    write!(xml, r#"</Objects><ObjectData Count="{}">"#, objects.len()).unwrap();
    for (index, object) in objects.iter().enumerate() {
        let mut properties = String::new();
        let mut count = 0;
        for &(property, target) in &object.links {
            write!(
                properties,
                r#"<Property name="{property}" type="App::PropertyLink"><Link value="{}"/></Property>"#,
                object_name(target)
            )
            .unwrap();
            count += 1;
        }
        if !object.members.is_empty() {
            write!(
                properties,
                r#"<Property name="Group" type="App::PropertyLinkList"><LinkList count="{}">"#,
                object.members.len()
            )
            .unwrap();
            for &member in &object.members {
                write!(properties, r#"<Link value="{}"/>"#, object_name(member)).unwrap();
            }
            properties.push_str("</LinkList></Property>");
            count += 1;
        }
        if !object.expressions.is_empty() {
            write!(
                properties,
                r#"<Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="{}">"#,
                object.expressions.len()
            )
            .unwrap();
            for (slot, &owner) in object.expressions.iter().enumerate() {
                write!(
                    properties,
                    r#"<Expression path="Length{slot}" expression="{}.Length"/>"#,
                    object_name(owner)
                )
                .unwrap();
            }
            properties.push_str("</ExpressionEngine></Property>");
            count += 1;
        }
        write!(
            xml,
            r#"<Object name="{}"><Properties Count="{count}">{properties}</Properties></Object>"#,
            object_name(index)
        )
        .unwrap();
    }
    xml.push_str("</ObjectData></Document>");
    xml
}

fn graph(document: &str) -> crate::persistence::Graph {
    let ctx = cadmpeg_test_support::service_decode_context();
    let xml = crate::container::admit_document(&ctx, document.as_bytes()).expect("document XML");
    crate::persistence::parse_document(&xml, crate::persistence::Vocabulary::Objects)
        .expect("persistence graph")
}

fn owned_ordering(result: Result<(HashMap<&str, u64>, BTreeSet<String>), CodecError>) -> Ordering {
    result
        .map(|(ordinals, cycle_affected)| {
            (
                ordinals
                    .into_iter()
                    .map(|(id, ordinal)| (id.to_owned(), ordinal))
                    .collect(),
                cycle_affected,
            )
        })
        .map_err(|error| error.to_string())
}

/// Ordering of `graph` by [`super::super::feature_ordinals`] and by the
/// pairwise selection it replaced.
fn orderings(graph: &crate::persistence::Graph) -> (Ordering, Ordering) {
    let mut properties_by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    for property in &graph.properties {
        properties_by_owner
            .entry(property.owner.as_str())
            .or_default()
            .push(property);
    }
    let mut parent_by_member = HashMap::new();
    for body in graph
        .objects
        .iter()
        .filter(|object| super::super::is_body(&object.type_name))
    {
        let Some(property) = properties_by_owner
            .get(body.id().as_str())
            .and_then(|properties| super::super::body_membership_property(properties))
        else {
            continue;
        };
        for member in property
            .links()
            .iter()
            .flatten()
            .filter_map(crate::native::LinkTarget::object)
        {
            parent_by_member.insert(member, super::super::feature_id(body).expect("feature id"));
        }
    }
    (
        owned_ordering(super::super::feature_ordinals(
            &graph.objects,
            &properties_by_owner,
            &parent_by_member,
        )),
        owned_ordering(pairwise_feature_ordinals(
            &graph.objects,
            &properties_by_owner,
            &parent_by_member,
        )),
    )
}

/// The selection `feature_ordinals` replaced: each step rescans every
/// un-emitted object for the earliest ready one.
fn pairwise_feature_ordinals<'a>(
    objects: &'a [ObjectRecord],
    properties_by_owner: &HashMap<&'a str, Vec<&'a PropertyRecord>>,
    parent_by_member: &HashMap<&'a str, FeatureId>,
) -> Result<(HashMap<&'a str, u64>, BTreeSet<String>), CodecError> {
    use super::super::{expression_identifiers, feature_id, is_body, is_design_object};
    let design_objects = objects
        .iter()
        .filter(|object| is_design_object(&object.type_name))
        .collect::<Vec<_>>();
    let mut object_by_id = HashMap::new();
    let mut object_by_name = HashMap::new();
    let mut object_by_feature = HashMap::new();
    let mut source_ordinals = Vec::new();
    for object in &design_objects {
        object_by_id.insert(object.id().as_str(), *object);
        object_by_name.insert(object.name().as_str(), *object);
        object_by_feature.insert(feature_id(object)?, object.id().as_str());
        source_ordinals.push(cadmpeg_core::decode::u64_from_index(object.order));
    }
    source_ordinals.sort_unstable_by(Ord::cmp);
    let mut emitted = BTreeSet::new();
    let mut ordinals = HashMap::new();
    let mut cycle_affected = BTreeSet::new();
    while emitted.len() < design_objects.len() {
        let ready = |object: &&ObjectRecord| {
            let id = object.id().as_str();
            let parent_ready = parent_by_member
                .get(id)
                .and_then(|parent| object_by_feature.get(parent))
                .is_none_or(|parent| emitted.contains(parent));
            let declared_ready = is_body(&object.type_name)
                || object.dependencies.iter().all(|dependency| {
                    !object_by_id.contains_key(dependency.as_str())
                        || emitted.contains(dependency.as_str())
                });
            let expressions_ready = properties_by_owner
                .get(id)
                .into_iter()
                .flatten()
                .flat_map(|property| property.values())
                .filter_map(|value| value.attributes.get("expression"))
                .flat_map(|expression| expression_identifiers(expression))
                .filter_map(|identifier| identifier.split_once('.').map(|(owner, _)| owner))
                .filter_map(|owner| object_by_name.get(owner))
                .all(|dependency| {
                    dependency.id() == object.id() || emitted.contains(dependency.id().as_str())
                });
            let links_ready = is_body(&object.type_name)
                || properties_by_owner
                    .get(id)
                    .into_iter()
                    .flatten()
                    .flat_map(|property| {
                        property
                            .links()
                            .iter()
                            .map(move |link| (property.name.as_str(), link))
                    })
                    .filter_map(|(name, link)| Some((name, link.as_ref()?.object()?)))
                    .filter(|(name, dependency)| {
                        object_by_id.get(dependency).is_some_and(|dependency| {
                            matches!(
                                *name,
                                "Base"
                                    | "BaseFeature"
                                    | "Originals"
                                    | "Path"
                                    | "Profile"
                                    | "Sketch"
                                    | "Sections"
                                    | "Source"
                                    | "Spine"
                            ) || dependency.order < object.order
                        })
                    })
                    .all(|(_, dependency)| emitted.contains(dependency));
            parent_ready && declared_ready && expressions_ready && links_ready
        };
        let unemitted = || {
            design_objects
                .iter()
                .copied()
                .filter(|object| !emitted.contains(object.id().as_str()))
        };
        let next = if let Some(next) = unemitted().filter(ready).min_by_key(|object| object.order) {
            next
        } else {
            for object in unemitted() {
                cycle_affected.insert(object.id().to_owned());
            }
            unemitted()
                .min_by_key(|object| object.order)
                .ok_or_else(|| CodecError::malformed("lost an un-emitted object"))?
        };
        let ordinal = source_ordinals[ordinals.len()];
        emitted.insert(next.id().as_str());
        ordinals.insert(next.id().as_str(), ordinal);
    }
    Ok((ordinals, cycle_affected))
}

#[test]
fn design_object_ordering_matches_the_pairwise_selection() {
    const TYPES: [&str; 5] = [
        "PartDesign::Body",
        "PartDesign::Pad",
        "Sketcher::SketchObject",
        "PartDesign::Feature",
        "App::DocumentObjectGroup",
    ];
    const LINKS: [&str; 5] = ["Profile", "Base", "Sketch", "Support", "Reference"];
    let mut random = Random(0x2545_f491_4f6c_dd1d);
    let mut cycles = 0;
    for case in 0..120 {
        let count = 2 + random.below(40);
        let acyclic = case % 2 == 0;
        let mut objects = (0..count)
            .map(|_| GeneratedObject::new(TYPES[random.below(TYPES.len())]))
            .collect::<Vec<_>>();
        for (index, object) in objects.iter_mut().enumerate() {
            for _ in 0..random.below(3) {
                let dependency = random.below(count);
                if dependency != index && (!acyclic || dependency > index) {
                    object.dependencies.insert(dependency);
                }
            }
            for link in LINKS {
                if random.below(4) == 0 {
                    object.links.push((link, random.below(count)));
                }
            }
            for _ in 0..random.below(3) {
                let owner = random.below(count);
                if !acyclic || owner >= index {
                    object.expressions.push(owner);
                }
            }
            if object.type_name == "PartDesign::Body" {
                for _ in 0..random.below(4) {
                    object.members.insert(random.below(count));
                }
            }
        }
        let graph = graph(&generated_document(&objects));
        let (kahn, pairwise) = orderings(&graph);
        assert_eq!(kahn, pairwise, "case {case}");
        cycles += usize::from(kahn.is_ok_and(|(_, cycle_affected)| !cycle_affected.is_empty()));
    }
    assert!(cycles > 10, "only {cycles} cases exercised a cycle");
}

#[test]
fn design_object_ordering_is_linear_in_a_reverse_dependency_chain() {
    // Each object declares the next, so the pairwise selection rescans every
    // remaining object per step: about 2^27 readiness checks.
    const OBJECTS: usize = 1 << 14;
    let objects = (0..OBJECTS)
        .map(|index| {
            let mut object = GeneratedObject::new("PartDesign::Feature");
            if index + 1 < OBJECTS {
                object.dependencies.insert(index + 1);
            }
            object
        })
        .collect::<Vec<_>>();
    let graph = graph(&generated_document(&objects));
    let mut properties_by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    for property in &graph.properties {
        properties_by_owner
            .entry(property.owner.as_str())
            .or_default()
            .push(property);
    }
    let (ordinals, cycle_affected) =
        super::super::feature_ordinals(&graph.objects, &properties_by_owner, &HashMap::new())
            .expect("acyclic chain");
    let mut source_ordinals = graph
        .objects
        .iter()
        .map(|object| cadmpeg_core::decode::u64_from_index(object.order))
        .collect::<Vec<_>>();
    source_ordinals.sort_unstable();
    assert!(cycle_affected.is_empty());
    for (index, object) in graph.objects.iter().enumerate() {
        assert_eq!(
            ordinals[object.id().as_str()],
            source_ordinals[OBJECTS - 1 - index]
        );
    }
}
