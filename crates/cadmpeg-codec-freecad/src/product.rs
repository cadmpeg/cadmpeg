// SPDX-License-Identifier: Apache-2.0
//! Product containers and link occurrences recovered from the application graph.

use crate::loss::FreecadLossCode;
use crate::native::frame::FiniteFrame;
use crate::native::joint::JointRecord;
use crate::placement::{placement_components, placement_matrix};
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::ids::Identity;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::unknown::part::{Cause, GivenUpRecord};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::num::{NonZeroU64, NonZeroUsize};

use crate::brep::ShapePayloadRecord;
use crate::layout::link_array_side_entry_header as link_array;
use crate::native::{
    malformed, parse_bool, ContainerNode, CopyOnChangePolicy as NativeCopyOnChangePolicy,
    LinkArrayCardinality, LinkOccurrence, ObjectRecord, ProductNode, ProductNodeRecord,
    PropertyRecord,
};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{
    ExternalDocument, Occurrence, ProductDefinition, ProductDefinitionKind, Storage,
};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::Body;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::FiniteVector;

/// Elements one restored link array may contain. A larger count is malformed.
// resource-cap: MAX_LINK_ARRAY_ELEMENTS = 2
const MAX_LINK_ARRAY_ELEMENTS: usize = 1_000_000;

/// Records a give-up names, such as the occurrences it leaves with no
/// transform; the rest are counted.
// resource-cap: MAX_NAMED_RECORDS = 4
const MAX_NAMED_RECORDS: usize = 8;

/// The product records of a document, and the product objects among them
/// whose placement, scale, or element placements are stored in a form the
/// decoder cannot read.
pub(crate) struct ProductNodes {
    pub(crate) records: Vec<ProductNodeRecord>,
    pub(crate) unplaced: HashSet<String>,
    /// The links given up for a member or element list stored in a form the
    /// decoder cannot read.
    pub(crate) unresolved: Vec<UnresolvedLink>,
    /// The links whose `LinkedObject` is stored in a form the decoder cannot
    /// read.
    unknown_prototypes: HashSet<String>,
    /// Removed link elements, keyed to the link whose carrier gave them up.
    dropped_elements: HashMap<String, String>,
    /// The give-ups of placement values, charged by [`transfer_neutral`]
    /// once it knows the occurrences each leaves with no transform.
    pub(crate) deferred: Vec<Deferred>,
}

/// A link with no product record, and the placement it stores.
pub(crate) struct UnresolvedLink {
    pub(crate) object: String,
    pub(crate) placement: Option<FiniteFrame>,
}

/// The give-up of a value of object `object` that a transform composes.
pub(crate) struct Deferred {
    object: String,
    record: GivenUpRecord,
}

/// A link given up for a member or element list stored in a form the
/// decoder cannot read, and the properties given up with it.
struct DroppedLink<'a> {
    object: &'a ObjectRecord,
    /// The objects its stored `ElementList` names, as far as it reads.
    listed: Vec<String>,
    given_up: Vec<(&'a PropertyRecord, CodecError)>,
}

/// A property value of one object.
enum Stored<'a, T> {
    Absent,
    Read(T),
    /// The property is stored in a form the decoder cannot read.
    Unreadable(&'a PropertyRecord, CodecError),
}

impl<T> Stored<'_, T> {
    fn read(self) -> Option<T> {
        match self {
            Self::Read(value) => Some(value),
            Self::Absent | Self::Unreadable(..) => None,
        }
    }

    /// The value read, with an unreadable property reported to `losses`.
    fn reported(self, losses: &mut Vec<LossNote>) -> Result<Option<T>, CodecError> {
        match self {
            Self::Absent => Ok(None),
            Self::Read(value) => Ok(Some(value)),
            Self::Unreadable(property, error) => {
                give_up(losses, property, error, None)?;
                Ok(None)
            }
        }
    }

    /// The value read, with the give-up of an unreadable property kept in
    /// `deferred`.
    fn deferred(self, deferred: &mut Vec<Deferred>) -> Result<Option<T>, CodecError> {
        match self {
            Self::Absent => Ok(None),
            Self::Read(value) => Ok(Some(value)),
            Self::Unreadable(property, error) => {
                deferred.push(Deferred {
                    object: property.owner.clone(),
                    record: given_up(property, error, None)?,
                });
                Ok(None)
            }
        }
    }
}

/// Reads property `name` of an object whose properties are `owned`.
fn stored<'a, T>(
    owned: &[&'a PropertyRecord],
    name: &str,
    read: impl FnOnce(&'a PropertyRecord) -> Result<T, CodecError>,
) -> Stored<'a, T> {
    let Some(property) = owned.iter().copied().find(|property| property.name == name) else {
        return Stored::Absent;
    };
    match read(property) {
        Ok(value) => Stored::Read(value),
        Err(error) => Stored::Unreadable(property, error),
    }
}

/// Reports property `property`, given up for `error`, and the record
/// `dropped` that the decoder builds only from it (§3.3).
pub(crate) fn give_up(
    losses: &mut Vec<LossNote>,
    property: &PropertyRecord,
    error: CodecError,
    dropped: Option<&str>,
) -> Result<(), CodecError> {
    losses.push(given_up(property, error, dropped)?.loss());
    Ok(())
}

/// The give-up of property `property` for `error`, which [`give_up`] reports.
fn given_up(
    property: &PropertyRecord,
    error: CodecError,
    dropped: Option<&str>,
) -> Result<GivenUpRecord, CodecError> {
    if matches!(error, CodecError::ResourceLimit(_)) {
        return Err(error);
    }
    let record = Identity::try_from(property.id.as_str()).map_err(CodecError::malformed)?;
    let given_up = GivenUpRecord::new(
        record,
        Cause::Unreadable.of(&error),
        format_args!("object {}: {error}", property.owner),
    );
    Ok(match dropped {
        Some(dropped) => {
            given_up.dropping([Identity::try_from(dropped).map_err(CodecError::malformed)?])
        }
        None => given_up,
    })
}

pub(crate) fn transfer(
    ctx: &DecodeContext<'_>,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    entries: &BTreeMap<String, View<'_>>,
    losses: &mut Vec<LossNote>,
) -> Result<ProductNodes, CodecError> {
    let mut by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    for property in properties {
        if !by_owner.contains_key(property.owner.as_str()) {
            by_owner.insert(&property.owner, Vec::new());
        }
        if let Some(owned) = by_owner.get_mut(property.owner.as_str()) {
            owned.push(property);
        }
    }
    let mut output = Vec::new();
    let mut unplaced = HashSet::new();
    let mut unresolved = Vec::new();
    let mut unknown_prototypes = HashSet::new();
    let mut dropped_links = Vec::new();
    let mut deferred = Vec::new();
    for object in objects {
        let Some(kind) = product_kind(&object.type_name) else {
            continue;
        };
        let source = by_owner
            .get(object.id().as_str())
            .map_or(&[][..], Vec::as_slice);
        let mut owned = Vec::new();
        owned.extend_from_slice(source);
        let id = crate::native::native_id("product", object.name());
        let occurrence = kind == ProductKind::Occurrence;
        let mut membership = Vec::new();
        let members = sole(
            stored(&owned, "Group", |property| {
                link_list(property, "App::PropertyLinkList", "Group").map(linked_object_names)
            }),
            &mut membership,
        )
        .unwrap_or_default();
        let element_objects = sole(
            stored(&owned, "ElementList", |property| {
                let names = linked_object_names(link_list(
                    property,
                    "App::PropertyLinkList",
                    "ElementList",
                )?);
                if names.len() > MAX_LINK_ARRAY_ELEMENTS {
                    return Err(malformed(format!(
                        "ElementList lists {} objects, more than {MAX_LINK_ARRAY_ELEMENTS}",
                        names.len()
                    )));
                }
                Ok(names)
            }),
            &mut membership,
        )
        .unwrap_or_default();
        let mut prototype_unreadable = Vec::new();
        let mut copy_unreadable = Vec::new();
        let (prototype_link, copy_on_change_source, copy_on_change_group) = if occurrence {
            (
                linked_target(
                    &owned,
                    ("LinkedObject", "App::PropertyXLink", "XLink"),
                    &mut prototype_unreadable,
                ),
                linked_target(
                    &owned,
                    ("LinkCopyOnChangeSource", "App::PropertyXLink", "XLink"),
                    &mut copy_unreadable,
                ),
                linked_target(
                    &owned,
                    ("LinkCopyOnChangeGroup", "App::PropertyLink", "Link"),
                    &mut copy_unreadable,
                ),
            )
        } else {
            (None, None, None)
        };
        let placed = object_placement(&owned, losses, &mut deferred)?;
        // A link built without its member or element list places the wrong
        // objects.
        if occurrence && !membership.is_empty() {
            unresolved.push(UnresolvedLink {
                object: object.id().to_owned(),
                placement: match placed {
                    Placed::At(_, frame) => Some(frame),
                    Placed::Unstated | Placed::Unknown => None,
                },
            });
            unplaced.insert(object.id().to_owned());
            membership.extend(prototype_unreadable);
            membership.extend(copy_unreadable);
            dropped_links.push(DroppedLink {
                object,
                listed: match owned.iter().find(|property| property.name == "ElementList") {
                    Some(property) => dropped_element_names(ctx, property)?,
                    None => Vec::new(),
                },
                given_up: membership,
            });
            continue;
        }
        for (property, error) in prototype_unreadable {
            unknown_prototypes.insert(object.id().to_owned());
            deferred.push(Deferred {
                object: object.id().to_owned(),
                record: given_up(property, error, None)?.costing(format_args!(
                    "the occurrences of {} place an unresolved definition",
                    object.id()
                )),
            });
        }
        let copy_on_change_stated = copy_unreadable.is_empty();
        for (property, error) in copy_unreadable {
            losses.push(
                given_up(property, error, None)?
                    .costing(format_args!(
                        "the copy-on-change state of {} is not stated",
                        object.id()
                    ))
                    .loss(),
            );
        }
        // A container keeps its placement and lists no member.
        for (property, error) in membership {
            let listed = linked_object_names(property.links());
            let listed = if listed.is_empty() {
                String::new()
            } else {
                format!(" ({})", bounded_names(listed.iter().map(String::as_str)))
            };
            let consequence = if kind == ProductKind::Group {
                format!(
                    "the objects it lists{listed} are not members of {}",
                    object.id()
                )
            } else {
                format!(
                    "the objects it lists{listed} are placed without the placement of {}",
                    object.id()
                )
            };
            losses.push(given_up(property, error, None)?.costing(consequence).loss());
        }
        let (local_transform, placement_property) = match placed {
            Placed::Unstated => (None, None),
            Placed::At(property, frame) => (Some(frame), Some(property.id.clone())),
            Placed::Unknown => {
                unplaced.insert(object.id().to_owned());
                (None, None)
            }
        };
        let node = match kind {
            ProductKind::Occurrence => {
                let mut placeable = true;
                let scale =
                    placement_value(scale_property(&owned, true), &mut placeable, &mut deferred)?
                        .flatten();
                let element_transforms = placement_value(
                    stored(&owned, "PlacementList", |property| {
                        parse_placement_list(property, entries)
                    }),
                    &mut placeable,
                    &mut deferred,
                )?
                .unwrap_or_default();
                let element_scales = placement_value(
                    stored(&owned, "ScaleList", |property| {
                        parse_vector_list(property, entries)
                    }),
                    &mut placeable,
                    &mut deferred,
                )?
                .unwrap_or_default();
                if !placeable {
                    unplaced.insert(object.id().to_owned());
                }
                let element_count =
                    stored(&owned, "ElementCount", element_count).reported(losses)?;
                let element_visibility = stored(&owned, "VisibilityList", |property| {
                    bool_list(property, "VisibilityList")
                })
                .reported(losses)?
                .unwrap_or_default();
                stored(&owned, "ShowElement", |property| {
                    bool_value(property, "ShowElement")
                })
                .reported(losses)?;
                let claim_child = stored(&owned, "LinkClaimChild", |property| {
                    bool_value(property, "LinkClaimChild")
                })
                .reported(losses)?;
                let copy_on_change_policy =
                    stored(&owned, "LinkCopyOnChange", copy_on_change_value).reported(losses)?;
                let copy_on_change_touched =
                    stored(&owned, "LinkCopyOnChangeTouched", |property| {
                        bool_value(property, "LinkCopyOnChangeTouched")
                    })
                    .reported(losses)?;
                let admitted = if copy_on_change_stated {
                    crate::native::CopyOnChange::from_admitted(
                        copy_on_change_policy,
                        copy_on_change_source,
                        copy_on_change_group,
                        copy_on_change_touched,
                    )
                } else {
                    Ok(None)
                };
                let copy_on_change = match admitted {
                    Ok(copy_on_change) => copy_on_change,
                    Err(error) => {
                        let payload = [
                            "LinkCopyOnChangeSource",
                            "LinkCopyOnChangeGroup",
                            "LinkCopyOnChangeTouched",
                        ]
                        .into_iter()
                        .find_map(|name| {
                            owned.iter().copied().find(|property| property.name == name)
                        });
                        if let Some(property) = payload {
                            give_up(losses, property, malformed(error), None)?;
                        }
                        None
                    }
                };
                ProductNode::Occurrence(Box::new(LinkOccurrence {
                    members,
                    prototype: prototype_link
                        .as_ref()
                        .and_then(|link| link.object())
                        .map(ToOwned::to_owned),
                    external_document: prototype_link
                        .as_ref()
                        .and_then(|link| link.document())
                        .cloned(),
                    local_transform,
                    placement_property,
                    array: crate::native::LinkArray::new(
                        element_count,
                        element_transforms,
                        element_scales,
                        element_visibility,
                        element_objects,
                    ),
                    link_transform: stored(&owned, "LinkTransform", |property| {
                        bool_value(property, "LinkTransform")
                    })
                    .read(),
                    linked_subelements: prototype_link
                        .as_ref()
                        .map(|link| nonempty_subelements(link.subelements()))
                        .unwrap_or_default(),
                    claim_child,
                    copy_on_change,
                    scale,
                }))
            }
            ProductKind::Group => ProductNode::Group(ContainerNode {
                members,
                local_transform,
                placement_property,
            }),
            ProductKind::Part => ProductNode::Part(ContainerNode {
                members,
                local_transform,
                placement_property,
            }),
            ProductKind::LinkGroup => ProductNode::LinkGroup {
                container: ContainerNode {
                    members,
                    local_transform,
                    placement_property,
                },
                element_objects,
            },
        };
        output.push(ProductNodeRecord {
            id,
            object: object.id().to_owned(),
            node,
        });
    }
    let dropped_elements = drop_link_elements(
        objects,
        &by_owner,
        dropped_links,
        &mut output,
        &mut deferred,
    )?;
    Ok(ProductNodes {
        records: output,
        unplaced,
        unresolved,
        unknown_prototypes,
        dropped_elements,
        deferred,
    })
}

fn dropped_element_names(
    ctx: &DecodeContext<'_>,
    property: &PropertyRecord,
) -> Result<Vec<String>, CodecError> {
    if !property.links().is_empty() {
        return Ok(linked_object_names(property.links()));
    }
    let xml = match ctx.parse_xml(property.xml.text(), "FreeCAD dropped link's element list") {
        Ok(tree) => tree,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(Vec::new()),
    };
    Ok(xml
        .root_element()
        .children()
        .filter(|root| root.has_tag_name("LinkList"))
        .flat_map(|root| root.children())
        .filter(|child| child.has_tag_name("Link"))
        .filter_map(|child| child.attribute("value"))
        .filter(|name| !name.is_empty())
        .map(|name| crate::native::native_id("object", name))
        .collect())
}

/// Gives up each link of `dropped` with the `App::LinkElement` objects it
/// holds: those its stored `ElementList` names and those whose `_LinkOwner`
/// is its persisted id, other than the elements a kept link lists. Their
/// records leave `output`; each give-up is kept in `deferred`.
fn drop_link_elements(
    objects: &[ObjectRecord],
    by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
    dropped: Vec<DroppedLink<'_>>,
    output: &mut Vec<ProductNodeRecord>,
    deferred: &mut Vec<Deferred>,
) -> Result<HashMap<String, String>, CodecError> {
    if dropped.is_empty() {
        return Ok(HashMap::new());
    }
    let dropped_objects = dropped
        .iter()
        .map(|link| link.object.id().as_str())
        .collect::<HashSet<_>>();
    let kept = listed_link_element_objects(output, objects);
    let mut elements = HashMap::new();
    let mut elements_by_owner = HashMap::<i64, Vec<&ObjectRecord>>::new();
    for object in objects
        .iter()
        .filter(|object| object.type_name == "App::LinkElement")
    {
        if kept.contains(object.id().as_str()) || dropped_objects.contains(object.id().as_str()) {
            continue;
        }
        elements.insert(object.id().as_str(), object);
        if let Some(owner) = by_owner
            .get(object.id().as_str())
            .and_then(|owned| owned.iter().find(|property| property.name == "_LinkOwner"))
            .and_then(|property| link_owner(property).ok())
            .filter(|&owner| owner != 0)
        {
            elements_by_owner.entry(owner).or_default().push(object);
        }
    }
    let mut removed = HashMap::new();
    for link in dropped {
        let mut ids = vec![crate::native::native_id("product", link.object.name())];
        let owned = link
            .object
            .persistent_id
            .and_then(|id| elements_by_owner.get(&id));
        for element in link
            .listed
            .iter()
            .filter_map(|id| elements.get(id.as_str()).copied())
            .chain(owned.into_iter().flatten().copied())
        {
            if let std::collections::hash_map::Entry::Vacant(slot) =
                removed.entry(element.id().clone())
            {
                ids.push(crate::native::native_id("product", element.name()));
                slot.insert(link.object.id().clone());
            }
        }
        let mut ids = Some(ids);
        for (property, error) in link.given_up {
            let record = given_up(property, error, None)?;
            deferred.push(Deferred {
                object: link.object.id().clone(),
                record: match ids.take() {
                    Some(ids) => record.dropping(
                        ids.iter()
                            .map(|id| Identity::try_from(id.as_str()))
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(CodecError::malformed)?,
                    ),
                    None => record,
                },
            });
        }
    }
    output.retain(|record| !removed.contains_key(&record.object));
    Ok(removed)
}

/// The target of link property `name` of type `expected_type` and root
/// element `root`, with an unreadable property kept in `unreadable`.
fn linked_target<'a>(
    owned: &[&'a PropertyRecord],
    (name, expected_type, root): (&str, &str, &str),
    unreadable: &mut Vec<(&'a PropertyRecord, CodecError)>,
) -> Option<crate::native::LinkTarget> {
    sole(
        stored(owned, name, |property| {
            Ok(single_link(property, expected_type, root, name)?.cloned())
        }),
        unreadable,
    )
    .flatten()
}

/// The persisted id of the link that holds link element `property`'s owner.
fn link_owner(property: &PropertyRecord) -> Result<i64, CodecError> {
    let value = single_value(property, "App::PropertyInteger", "_LinkOwner", "Integer")?;
    value
        .attributes
        .get("value")
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "product property {} has no integer value",
                property.id
            ))
        })
}

/// `names`, the first [`MAX_NAMED_RECORDS`] of them named and the rest
/// counted.
fn bounded_names<'a>(names: impl ExactSizeIterator<Item = &'a str>) -> String {
    let count = names.len();
    let named = names.take(MAX_NAMED_RECORDS).collect::<Vec<_>>().join(", ");
    match count.checked_sub(MAX_NAMED_RECORDS) {
        Some(rest) if rest > 0 => format!("{named} and {rest} more"),
        _ => named,
    }
}

/// The value of `value`, a placement value of a link, with the give-up of an
/// unreadable property kept in `deferred` and the link marked not
/// `placeable`.
fn placement_value<T>(
    value: Stored<'_, T>,
    placeable: &mut bool,
    deferred: &mut Vec<Deferred>,
) -> Result<Option<T>, CodecError> {
    if matches!(value, Stored::Unreadable(..)) {
        *placeable = false;
    }
    value.deferred(deferred)
}

/// The value of `value`, with an unreadable property kept in `unreadable`.
fn sole<'a, T>(
    value: Stored<'a, T>,
    unreadable: &mut Vec<(&'a PropertyRecord, CodecError)>,
) -> Option<T> {
    match value {
        Stored::Absent => None,
        Stored::Read(value) => Some(value),
        Stored::Unreadable(property, error) => {
            unreadable.push((property, error));
            None
        }
    }
}

/// The placement of an object whose properties are `owned`.
enum Placed<'a> {
    Unstated,
    At(&'a PropertyRecord, FiniteFrame),
    /// The object stores a placement in a form the decoder cannot read.
    Unknown,
}

/// The placement an object whose properties are `owned` stores. Each
/// unreadable `LinkTransform` is reported to `losses`; the give-up of each
/// unreadable placement property is kept in `deferred`. `FreeCAD` places an
/// object at its `LinkPlacement` where it has one, and `Placement` is an
/// alias of it, so either stands for the other where one is unreadable.
/// `LinkTransform` selects no carrier.
fn object_placement<'a>(
    owned: &[&'a PropertyRecord],
    losses: &mut Vec<LossNote>,
    deferred: &mut Vec<Deferred>,
) -> Result<Placed<'a>, CodecError> {
    let frame = |property: &'a PropertyRecord| {
        placement_matrix(property)?
            .map(|frame| (property, frame))
            .ok_or_else(|| malformed(format!("{} stores no placement", property.id)))
    };
    stored(owned, "LinkTransform", |property| {
        bool_value(property, "LinkTransform")
    })
    .reported(losses)?;
    let mut unknown = false;
    let mut read = |name: &str| {
        let value = stored(owned, name, frame);
        unknown |= matches!(value, Stored::Unreadable(..));
        value.deferred(deferred)
    };
    let link_placement = read("LinkPlacement")?;
    let placement = read("Placement")?;
    Ok(match (link_placement, placement) {
        (Some((property, link_placement)), placement) => {
            if let Some((stale, _)) =
                placement.filter(|(_, placement)| *placement != link_placement)
            {
                losses.push(
                    FreecadLossCode::AssemblyPlacementAliasNoncanonical.note(format!(
                        "object {} stores Placement with a value other than its LinkPlacement; \
                     the decoder applies LinkPlacement, which FreeCAD places it at, and \
                     the Placement value stays in property {}",
                        stale.owner, stale.id
                    )),
                );
            }
            Placed::At(property, link_placement)
        }
        (None, Some((property, placement))) => Placed::At(property, placement),
        (None, None) if unknown => Placed::Unknown,
        (None, None) => Placed::Unstated,
    })
}

fn linked_object_names(links: &[Option<crate::native::LinkTarget>]) -> Vec<String> {
    links
        .iter()
        .flatten()
        .filter_map(|link| link.object().map(ToOwned::to_owned))
        .collect()
}

fn nonempty_subelements(values: &[String]) -> Vec<String> {
    values
        .iter()
        .filter(|value| !value.is_empty())
        .cloned()
        .collect()
}

fn product_record_index(
    records: &[ProductNodeRecord],
) -> Result<HashMap<&str, &ProductNodeRecord>, CodecError> {
    let mut index = HashMap::new();
    for record in records {
        if index.insert(record.object.as_str(), record).is_some() {
            return Err(CodecError::malformed(format_args!(
                "product object {} has duplicate product records",
                record.object
            )));
        }
    }
    Ok(index)
}

/// The neutral product records of a document, and the container occurrence
/// each occurrence sits in, which placement composition walks.
#[derive(Default)]
pub(crate) struct Products {
    pub(crate) definitions: Vec<ProductDefinition>,
    pub(crate) occurrences: Vec<Occurrence>,
    pub(crate) parents: HashMap<OccurrenceId, OccurrenceId>,
}

/// The records [`transfer_neutral`] projects.
pub(crate) struct Sources<'a> {
    pub(crate) nodes: &'a ProductNodes,
    pub(crate) joints: &'a [JointRecord],
    pub(crate) objects: &'a [ObjectRecord],
    pub(crate) properties: &'a [PropertyRecord],
    pub(crate) payloads: &'a [ShapePayloadRecord],
    pub(crate) bodies: &'a [Body],
    pub(crate) tessellations: &'a [Tessellation],
}

/// Project the lossless native product records into reusable definitions and
/// the occurrences each places. The document is the root definition: it
/// places the objects no container holds. The link state of an `App::Link`
/// stays in its native record. An occurrence whose placement depends on a
/// value the decoder cannot read states no transform.
///
/// The give-ups in `deferred` are reported to `losses`, each naming the
/// occurrences it leaves with no transform.
///
/// # Errors
///
/// A product structure the decoder cannot project: duplicate product records
/// or a link array above its element limit. A resource refusal.
pub(crate) fn transfer_neutral(
    ctx: &DecodeContext<'_>,
    sources: &Sources<'_>,
    deferred: Vec<Deferred>,
    losses: &mut Vec<LossNote>,
) -> Result<Products, CodecError> {
    let mut deferred = deferred;
    let mut unplaced_by = HashMap::new();
    let products = project(ctx, sources, &mut deferred, &mut unplaced_by, losses);
    for Deferred { object, record } in deferred {
        let record = match unplaced_by.get(&object) {
            Some(occurrences) if products.is_ok() => record.costing(no_transform(occurrences)),
            _ => record,
        };
        losses.push(record.loss());
    }
    products
}

/// The consequence that occurrences `occurrences` state no transform.
fn no_transform(occurrences: &[OccurrenceId]) -> String {
    let named = bounded_names(occurrences.iter().map(OccurrenceId::as_str));
    if occurrences.len() == 1 {
        format!("occurrence {named} states no transform")
    } else {
        format!("occurrences {named} state no transform")
    }
}

/// The body of [`transfer_neutral`]. Each occurrence that states no transform
/// because a value of an object is unknown joins that object's list in
/// `unplaced_by`.
fn project(
    ctx: &DecodeContext<'_>,
    sources: &Sources<'_>,
    deferred: &mut Vec<Deferred>,
    unplaced_by: &mut HashMap<String, Vec<OccurrenceId>>,
    losses: &mut Vec<LossNote>,
) -> Result<Products, CodecError> {
    let Sources {
        nodes,
        joints,
        objects,
        properties,
        payloads,
        bodies,
        tessellations,
    } = *sources;
    let records = nodes.records.as_slice();
    let record_by_object = product_record_index(records)?;
    let object_by_id = objects
        .iter()
        .map(|object| (object.id().as_str(), object))
        .collect::<HashMap<_, _>>();
    // An assembly lists its joints and its joint group among its members;
    // they constrain the components and are not components themselves.
    let constraint_objects = joints
        .iter()
        .map(JointRecord::object)
        .chain(
            objects
                .iter()
                .filter(|object| object.type_name == "Assembly::JointGroup")
                .map(|object| object.id().as_str()),
        )
        .collect::<HashSet<_>>();
    // A datum is reference geometry, a feature and not a component (§6.2):
    // a container that lists it does not place it, and its `Shape` is its
    // display shape, which no definition owns.
    let datum_objects = objects
        .iter()
        .filter(|object| crate::design::is_datum_object(&object.type_name))
        .map(|object| object.id().as_str())
        .collect::<HashSet<_>>();
    // Links place their own occurrences, also where the decoder gave up
    // their record.
    let link_objects = objects
        .iter()
        .filter(|object| product_kind(&object.type_name) == Some(ProductKind::Occurrence))
        .map(|object| object.id().as_str())
        .collect::<HashSet<_>>();
    let dropped_link =
        |object: &str| link_objects.contains(object) && !record_by_object.contains_key(object);
    let mut properties_by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    let mut owners = Vec::new();
    for property in properties {
        if !properties_by_owner.contains_key(property.owner.as_str()) {
            properties_by_owner.insert(property.owner.as_str(), Vec::new());
            owners.push(property.owner.as_str());
        }
        if let Some(owned) = properties_by_owner.get_mut(property.owner.as_str()) {
            owned.push(property);
        }
    }
    let parent_map = ParentMap::new(records, objects, &properties_by_owner);
    let mut component_objects = Vec::new();
    // Components placed by a container of their own; the rest are reached
    // only as a link's prototype and are placed by those links. An object a
    // `PartDesign::Body` holds is placed only through the Body.
    let mut placed_objects = HashSet::new();
    let mut link_prototypes = HashSet::new();
    for record in records {
        if !matches!(record.node, ProductNode::Occurrence(_)) {
            component_objects.push(record.object.as_str());
            if parent_map.has_product_placement(&record.object) {
                placed_objects.insert(record.object.as_str());
            }
        }
    }
    for record in records {
        for member in record.members().iter().filter(|member| {
            !link_objects.contains(member.as_str())
                && !constraint_objects.contains(member.as_str())
                && !datum_objects.contains(member.as_str())
                && parent_map.has_product_placement(member)
        }) {
            component_objects.push(member.as_str());
            placed_objects.insert(member.as_str());
        }
        if record.external_document().is_none() {
            if let Some(prototype) = record.prototype() {
                component_objects.push(prototype);
                link_prototypes.insert(prototype);
            }
        }
        for target in [
            record.copy_on_change_source(),
            record.copy_on_change_group(),
        ]
        .into_iter()
        .flatten()
        {
            if target.document().is_none() {
                if let Some(name) = target.object() {
                    component_objects.push(name);
                }
            }
        }
        for name in record.element_objects() {
            component_objects.push(name);
            if matches!(record.node, ProductNode::LinkGroup { .. })
                && object_by_id.contains_key(name.as_str())
                && !link_objects.contains(name.as_str())
                && !constraint_objects.contains(name.as_str())
            {
                placed_objects.insert(name.as_str());
            }
        }
    }
    for joint in joints {
        for reference in joint.references() {
            if reference.document().is_none() {
                if let Some(name) = reference.object().filter(|name| {
                    !link_objects.contains(*name) && parent_map.has_product_placement(name)
                }) {
                    component_objects.push(name);
                    placed_objects.insert(name);
                }
            }
        }
    }
    component_objects.retain(|object| !dropped_link(object));
    component_objects.sort_unstable_by(Ord::cmp);
    component_objects.dedup();

    let mut placements_by_object = HashMap::new();
    let mut unplaced = nodes
        .unplaced
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    for owner in owners {
        if let Some(record) = record_by_object.get(owner) {
            if let Some(frame) = record.local_transform() {
                placements_by_object.insert(owner, frame.transform());
            }
            continue;
        }
        // `transfer` read and gave up the placement of a product object it
        // states no record for.
        if object_by_id
            .get(owner)
            .is_some_and(|object| product_kind(&object.type_name).is_some())
        {
            continue;
        }
        let owned = properties_by_owner
            .get(owner)
            .map_or(&[][..], Vec::as_slice);
        match object_placement(owned, losses, deferred)? {
            Placed::Unstated => {}
            Placed::At(_, frame) => {
                placements_by_object.insert(owner, frame.transform());
            }
            Placed::Unknown => {
                unplaced.insert(owner);
            }
        }
    }
    let property_by_id = properties
        .iter()
        .map(|property| (property.id.as_str(), property))
        .collect::<HashMap<_, _>>();
    // An object's shape is its `Shape` property. Other payloads, such as a PartDesign
    // feature's `AddSubShape`, are not bodies of its definition.
    let mut body_owners = Vec::new();
    for payload in payloads {
        if let Some(property) = property_by_id
            .get(payload.property.as_str())
            .filter(|property| property.name == "Shape")
        {
            if !datum_objects.contains(property.owner.as_str()) {
                body_owners.push((
                    crate::native::model_id("body", &payload.id, ""),
                    property.owner.as_str(),
                ));
            }
        }
    }
    let mut bodies_by_owner = HashMap::<&str, Vec<&Body>>::new();
    for body in bodies {
        if let Some((_, owner)) = body_owners
            .iter()
            .find(|(prefix, _)| body.id.as_str().starts_with(prefix.as_str()))
        {
            bodies_by_owner.entry(owner).or_default().push(body);
        }
    }
    // FreeCAD restores the `Placement` of a `Shape` owner from the root location of its shape.
    for (&owner, owned) in &bodies_by_owner {
        let location = owned[0].transform.unwrap_or_default();
        if location.is_proper_rigid()
            && owned
                .iter()
                .all(|body| body.transform.unwrap_or_default() == location)
        {
            placements_by_object.insert(owner, location);
            if !dropped_link(owner) {
                unplaced.remove(owner);
            }
        }
    }
    // A copy-on-change group shows its children only through itself.
    let copy_on_change_groups = records
        .iter()
        .filter_map(ProductNodeRecord::copy_on_change_group)
        .filter(|target| target.document().is_none())
        .filter_map(|target| target.object())
        .collect::<HashSet<_>>();
    let copied_objects = records
        .iter()
        .filter(|record| copy_on_change_groups.contains(record.object.as_str()))
        .flat_map(ProductNodeRecord::element_objects)
        .map(String::as_str)
        .collect::<HashSet<_>>();

    let definition_id = |object: &str| -> Result<ProductDefinitionId, CodecError> {
        ProductDefinitionId::mint(crate::native::model_id(
            "product_definition",
            object,
            "definition",
        ))
        .map_err(CodecError::malformed)
    };
    let container_occurrence_id = |object: &str| -> Result<OccurrenceId, CodecError> {
        OccurrenceId::mint(crate::native::model_id("occurrence", object, "container"))
            .map_err(CodecError::malformed)
    };
    for (carrier, listed) in &parent_map.conflicts {
        let listed = listed
            .iter()
            .map(|(member, holder)| format!("{member}, which {holder} holds"))
            .collect::<Vec<_>>()
            .join("; ");
        give_up(
            losses,
            carrier,
            malformed(format!(
                "it lists {listed}; FreeCAD keeps each in the group that holds it, \
                 and this list places none of them"
            )),
            None,
        )?;
    }

    let element_objects = listed_link_element_objects(records, objects);
    let followed_cycles = followed_link_cycle_nodes(&record_by_object);
    for record in records
        .iter()
        .filter(|record| followed_cycles.contains(record.object.as_str()))
    {
        deferred.push(Deferred {
            object: record.object.clone(),
            record: GivenUpRecord::new(
                Identity::try_from(record.object.as_str()).map_err(CodecError::malformed)?,
                Cause::Unreadable,
                "its chain of links that follow their linked objects returns to it",
            ),
        });
    }
    let mut charged_links = HashSet::new();
    let mut occurrences = Vec::new();
    // The container object of each occurrence, parallel to `occurrences`.
    let mut containers: Vec<Option<&str>> = Vec::new();
    let mut externals = BTreeMap::<String, ProductDefinition>::new();
    for record in records.iter().filter(|record| {
        matches!(record.node, ProductNode::Occurrence(_))
            && !element_objects.contains(record.object.as_str())
    }) {
        let owned = properties_by_owner
            .get(record.object.as_str())
            .map_or(&[][..], Vec::as_slice);
        let cardinality = restored_cardinality(record, owned);
        let count = occurrence_count(record, cardinality)?.get();
        let is_array = matches!(cardinality, LinkArrayCardinality::Elements(_));
        let expanded = show_element(record, owned);
        let listing = parent_map.containers(record.object.as_str());
        if listing.is_empty() {
            continue;
        }
        let record_unplaced = unplaced.contains(record.object.as_str());
        let prototype_transform = linked_prototype_transform(
            ctx,
            record,
            &record_by_object,
            &Placements {
                placed: &placements_by_object,
                unplaced: &unplaced,
                unknown_prototypes: &nodes.unknown_prototypes,
                followed_cycles: &followed_cycles,
                dropped_elements: &nodes.dropped_elements,
            },
            &mut Links {
                charged: &mut charged_links,
                deferred,
            },
        )?;
        for index in 0..count {
            let element = count > 1;
            let element_object = (is_array && expanded)
                .then(|| record.element_objects().get(index))
                .flatten();
            // `None` where the element's placement is stored in a form the
            // decoder cannot read.
            let element_transform = match element_object {
                Some(object) if unplaced.contains(object.as_str()) => None,
                Some(object) => Some(
                    placements_by_object
                        .get(object.as_str())
                        .copied()
                        .unwrap_or_default(),
                ),
                None if !expanded && is_array => Some(
                    record
                        .element_transforms()
                        .get(index)
                        .map(|frame| frame.transform())
                        .unwrap_or_default(),
                ),
                None => Some(Transform::identity()),
            };
            let element_scale = match element_object {
                Some(object) => {
                    let element_properties = properties_by_owner
                        .get(object.as_str())
                        .map_or(&[][..], Vec::as_slice);
                    let restore_uniform = record_by_object
                        .get(object.as_str())
                        .is_some_and(|record| matches!(record.node, ProductNode::Occurrence(_)));
                    match scale_property(element_properties, restore_uniform) {
                        Stored::Absent => Some([1.0; 3]),
                        Stored::Read(scale) => Some(scale.map_or([1.0; 3], FiniteVector::get)),
                        Stored::Unreadable(property, error) => {
                            // `transfer` reports the scale of a link element.
                            if !restore_uniform {
                                deferred.push(Deferred {
                                    object: property.owner.clone(),
                                    record: given_up(property, error, None)?,
                                });
                            }
                            None
                        }
                    }
                }
                None if !expanded && is_array => Some(
                    record
                        .element_scales()
                        .get(index)
                        .copied()
                        .map_or([1.0; 3], FiniteVector::get),
                ),
                None => Some([1.0; 3]),
            };
            let child = if element {
                index.to_string()
            } else {
                "instance".to_owned()
            };
            let primary = OccurrenceId::mint(crate::native::model_id(
                "occurrence",
                &record.object,
                &child,
            ))
            .map_err(CodecError::malformed)?;
            let (transform, cause) = match (
                record_unplaced,
                element_transform,
                element_scale,
                &prototype_transform,
            ) {
                (true, ..) => (None, Some(record.object.as_str())),
                (false, None, ..) | (false, _, None, _) => {
                    (None, element_object.map(String::as_str))
                }
                (false, _, _, Prototype::Unknown(cause)) => (None, Some(cause.as_str())),
                (
                    false,
                    Some(element_transform),
                    Some(element_scale),
                    Prototype::At(prototype_transform),
                ) => {
                    let scale = link_scale_transform(record.scale().unwrap_or([1.0; 3]))?;
                    let element_scale = link_scale_transform(element_scale)?;
                    // A link that follows its object's placement is placed at
                    // its own placement composed after the object's.
                    let composed = record
                        .local_transform()
                        .map(FiniteFrame::transform)
                        .unwrap_or_default()
                        .compose(scale)
                        .and_then(|transform| transform.compose(element_transform))
                        .and_then(|transform| transform.compose(element_scale))
                        .and_then(|transform| transform.compose(*prototype_transform));
                    match composed {
                        Ok(transform) => (Some(transform), None),
                        Err(error) => {
                            let record = Identity::try_from(primary.as_str())
                                .map_err(CodecError::malformed)?;
                            losses.push(
                                GivenUpRecord::new(
                                    record,
                                    Cause::Undecoded,
                                    format_args!("its placement does not compose: {error}"),
                                )
                                .loss(),
                            );
                            (None, None)
                        }
                    }
                }
            };
            let prototype = if let Some(document) = record.external_document() {
                let document = external_document_reference(document.as_str(), document.attribute());
                let document = match record.prototype() {
                    Some(object) => document.with_object(object),
                    None => document,
                };
                external_definition(&mut externals, document)?
            } else if let Some(prototype) =
                record.prototype().filter(|object| !dropped_link(object))
            {
                definition_id(prototype)?
            } else {
                let unresolved = ProductDefinitionId::mint(crate::native::model_id(
                    "product_definition",
                    primary.as_str(),
                    "unresolved",
                ))
                .map_err(CodecError::malformed)?;
                externals.insert(
                    unresolved.as_str().to_owned(),
                    ProductDefinition::new(
                        unresolved.clone(),
                        Storage::External(ExternalDocument::default()),
                    ),
                );
                unresolved
            };
            for (position, &container) in listing.iter().enumerate() {
                let id = if position == 0 {
                    primary.clone()
                } else {
                    listed_occurrence_id(&record.object, &child, container)?
                };
                if let Some(cause) = cause.filter(|_| transform.is_none()) {
                    unplaced_by
                        .entry(cause.to_owned())
                        .or_default()
                        .push(id.clone());
                }
                let mut occurrence = Occurrence::new(id, prototype.clone(), transform);
                occurrence.name = Some(record.object.clone());
                occurrence.native_ref = Some(record.object.clone());
                occurrences.push(occurrence);
                containers.push(container);
            }
        }
    }

    let mut definitions = Vec::new();
    for &object in &component_objects {
        let record = record_by_object.get(object).copied();
        let kind = match record.map(|record| &record.node) {
            Some(ProductNode::Part(_)) => ProductDefinitionKind::Part,
            Some(ProductNode::Group(_)) => ProductDefinitionKind::Group,
            Some(ProductNode::LinkGroup { .. }) => ProductDefinitionKind::LinkGroup,
            _ => ProductDefinitionKind::Object,
        };
        let source_object = object_by_id.get(object).copied();
        let owned = properties_by_owner
            .get(object)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut bom_properties = BTreeMap::new();
        for name in [
            cadmpeg_core::nonblank_literal!("Label2"),
            cadmpeg_core::nonblank_literal!("StockCode"),
            cadmpeg_core::nonblank_literal!("Vendor"),
            cadmpeg_core::nonblank_literal!("Manufacturer"),
        ] {
            if let Some(value) = metadata_string(ctx, owned, name.as_str())? {
                bom_properties.insert(name, value);
            }
        }
        let part_runtime = source_object
            .is_some_and(|object| product_kind(&object.type_name) == Some(ProductKind::Part));
        if part_runtime {
            if let Some(value) = metadata_string(ctx, owned, "PartNumber")? {
                bom_properties.insert(cadmpeg_core::nonblank_literal!("PartNumber"), value);
            }
        }
        let part_number_carrier = if part_runtime { "Id" } else { "PartNumber" };
        let definition_bodies = bodies_by_owner
            .get(object)
            .map(|owned| owned.iter().map(|body| body.id.clone()).collect())
            .unwrap_or_default();
        definitions.push(ProductDefinition {
            sketches: Vec::default(),

            id: definition_id(object)?,
            kind: Some(kind),
            source_name: source_object.map(|object| object.name().to_owned()),
            label: metadata_string(ctx, owned, "Label")?,
            description: metadata_string(ctx, owned, "Description")?,
            part_number: metadata_string(ctx, owned, part_number_carrier)?
                .filter(|value| !value.is_empty()),
            bom_properties,
            bodies: definition_bodies,
            native_ref: Some((*object).to_owned()),
            generated_from: None,
            storage: cadmpeg_ir::products::Storage::Embedded,
            occurrences: Vec::new(),
            instance_bodies: Vec::new(),
            representations: Vec::new(),
            units: None,
            general_tolerance: None,
            active: None,
        });
    }

    // Definition bodies, instance bodies, and an object's mesh are in their
    // object's frame, so a link prototype with geometry needs its own occurrence
    // unless it is shown only through a listing object.
    let tessellation_ids = tessellations
        .iter()
        .map(|tessellation| tessellation.id.as_str())
        .collect::<HashSet<_>>();
    let mesh_owners = properties
        .iter()
        .filter(|property| {
            property.type_name == "Mesh::PropertyMeshKernel"
                && tessellation_ids.contains(format!("{}:mesh", property.id).as_str())
        })
        .map(|property| property.owner.as_str())
        .collect::<HashSet<_>>();
    for prototype in link_prototypes {
        let has_geometry = mesh_owners.contains(prototype)
            || definitions.iter().any(|definition| {
                definition.native_ref.as_deref() == Some(prototype)
                    && !(definition.bodies.is_empty() && definition.instance_bodies.is_empty())
            });
        if has_geometry
            && !link_objects.contains(prototype)
            && !copied_objects.contains(prototype)
            && parent_map.has_product_placement(prototype)
            && (placements_by_object.contains_key(prototype) || unplaced.contains(prototype))
        {
            placed_objects.insert(prototype);
        }
    }

    for object in component_objects
        .iter()
        .filter(|object| placed_objects.contains(**object))
    {
        let unknown = unplaced.contains(*object);
        let local_transform = (!unknown).then(|| {
            record_by_object
                .get(*object)
                .and_then(|record| record.local_transform())
                .map(FiniteFrame::transform)
                .or_else(|| placements_by_object.get(*object).copied())
                .unwrap_or_default()
        });
        for (position, container) in parent_map.containers(object).into_iter().enumerate() {
            let id = if position == 0 {
                container_occurrence_id(object)?
            } else {
                listed_occurrence_id(object, "container", container)?
            };
            if unknown {
                unplaced_by
                    .entry((*object).to_owned())
                    .or_default()
                    .push(id.clone());
            }
            let mut occurrence = Occurrence::new(id, definition_id(object)?, local_transform);
            occurrence.name = Some((*object).to_owned());
            occurrence.native_ref = Some((*object).to_owned());
            occurrences.push(occurrence);
            containers.push(container);
        }
    }
    let ordinals = {
        let mut by_source = HashMap::<&str, Vec<usize>>::new();
        for (index, occurrence) in occurrences.iter().enumerate() {
            if let Some(source) = occurrence.native_ref.as_deref() {
                by_source.entry(source).or_default().push(index);
            }
        }
        let mut assigned =
            ctx.alloc_filled(occurrences.len(), false, "freecad occurrence order")?;
        let mut ordinals =
            ctx.alloc_filled(occurrences.len(), 0_u32, "freecad occurrence ordinals")?;
        let mut next_ordinal = HashMap::<Option<String>, u32>::new();
        for &(container, ref members) in &parent_map.member_order {
            let mut ordinal = 0_u32;
            for member in members {
                for &index in by_source.get(member).into_iter().flatten() {
                    if assigned[index] || containers[index] != Some(container) {
                        continue;
                    }
                    assigned[index] = true;
                    ordinals[index] = ordinal;
                    ordinal = ordinal.checked_add(1).ok_or_else(|| {
                        CodecError::malformed("product occurrence ordinal exceeds u32")
                    })?;
                }
            }
            next_ordinal.insert(Some(container.to_owned()), ordinal);
        }
        for index in 0..occurrences.len() {
            if assigned[index] {
                continue;
            }
            let ordinal = next_ordinal
                .entry(containers[index].map(str::to_owned))
                .or_default();
            ordinals[index] = *ordinal;
            *ordinal = ordinal
                .checked_add(1)
                .ok_or_else(|| CodecError::malformed("product occurrence ordinal exceeds u32"))?;
        }
        ordinals
    };
    // Each container's definition, and the document for the rest, lists its
    // occurrences in member order.
    let mut order = (0..occurrences.len()).collect::<Vec<_>>();
    order.sort_by_key(|&index| (ordinals[index], index));
    let mut root = None::<ProductDefinition>;
    let mut parents = HashMap::new();
    for index in order {
        let id = occurrences[index].id.clone();
        match containers[index] {
            Some(container) => {
                let owner = definition_id(container)?;
                parents.insert(id.clone(), container_occurrence_id(container)?);
                if let Some(definition) = definitions.iter_mut().find(|row| row.id == owner) {
                    definition.occurrences.push(id);
                }
            }
            None => root
                .get_or_insert(ProductDefinition::root(
                    document_definition_id()?,
                    ExternalDocument::default(),
                ))
                .occurrences
                .push(id),
        }
    }
    definitions.extend(root);
    definitions.extend(externals.into_values());
    Ok(Products {
        definitions,
        occurrences,
        parents,
    })
}

/// The root definition: the document, which places the objects no container
/// holds.
pub(crate) fn document_definition_id() -> Result<ProductDefinitionId, CodecError> {
    ProductDefinitionId::mint(crate::native::model_id(
        "product_definition",
        "document",
        "root",
    ))
    .map_err(CodecError::malformed)
}

/// The definition of the document a link names, one per document and object.
fn external_definition(
    externals: &mut BTreeMap<String, ProductDefinition>,
    document: ExternalDocument,
) -> Result<ProductDefinitionId, CodecError> {
    let key = json::to_string(&document).map_err(CodecError::malformed)?;
    if let Some(definition) = externals.get(&key) {
        return Ok(definition.id.clone());
    }
    let id = ProductDefinitionId::mint(crate::native::model_id(
        "product_definition",
        "external",
        &key,
    ))
    .map_err(CodecError::malformed)?;
    let mut definition = ProductDefinition::new(id.clone(), Storage::External(document));
    definition.source_name = definition_source_name(&definition);
    externals.insert(key, definition);
    Ok(id)
}

fn definition_source_name(definition: &ProductDefinition) -> Option<String> {
    match &definition.storage {
        Storage::External(document) => document
            .object
            .as_ref()
            .map(|object| object.as_str().to_owned()),
        Storage::Embedded | Storage::File { .. } => None,
    }
}

/// Removes the shape payload's root location from the bodies of each product definition
/// that an occurrence uses.
///
/// The occurrences carry the object's placement, which is that root location. A definition
/// with no occurrence keeps the root location on its bodies.
pub(crate) fn localize_definition_bodies(
    definitions: &[ProductDefinition],
    occurrences: &[Occurrence],
    bodies: &mut [Body],
) {
    let used = occurrences
        .iter()
        .map(|occurrence| &occurrence.prototype)
        .collect::<HashSet<_>>();
    let localized = definitions
        .iter()
        .filter(|definition| used.contains(&definition.id))
        .flat_map(|definition| definition.bodies.iter())
        .collect::<HashSet<_>>();
    for body in bodies
        .iter_mut()
        .filter(|body| localized.contains(&body.id))
    {
        body.transform = None;
    }
}

/// The placements of the objects of a document, and the objects whose
/// placement is stored in a form the decoder cannot read.
struct Placements<'a> {
    placed: &'a HashMap<&'a str, Transform>,
    unplaced: &'a HashSet<&'a str>,
    /// The links whose `LinkedObject` is stored in a form the decoder cannot
    /// read.
    unknown_prototypes: &'a HashSet<String>,
    followed_cycles: &'a HashSet<&'a str>,
    dropped_elements: &'a HashMap<String, String>,
}

struct Links<'a> {
    charged: &'a mut HashSet<String>,
    deferred: &'a mut Vec<Deferred>,
}

/// The placement a link composes after its own.
#[derive(Clone)]
enum Prototype {
    At(Transform),
    /// Unknown because of a value of this object.
    Unknown(String),
}

/// The placement link `record` composes after its own: its linked object's,
/// after the one that object follows in turn. Unknown where one of them is
/// stored in a form the decoder cannot read or does not compose, or the
/// chain reaches a link of a cycle.
fn linked_prototype_transform(
    ctx: &DecodeContext<'_>,
    record: &ProductNodeRecord,
    records: &HashMap<&str, &ProductNodeRecord>,
    placements: &Placements<'_>,
    links: &mut Links<'_>,
) -> Result<Prototype, CodecError> {
    let _depth = ctx.enter_nested("resolve FCStd nested link transform")?;
    if record.link_transform() != Some(true) || record.external_document().is_some() {
        return Ok(Prototype::At(Transform::identity()));
    }
    let Some(prototype) = record.prototype() else {
        return Ok(if placements.unknown_prototypes.contains(&record.object) {
            Prototype::Unknown(record.object.clone())
        } else {
            Prototype::At(Transform::identity())
        });
    };
    if placements.followed_cycles.contains(record.object.as_str()) {
        return Ok(Prototype::Unknown(record.object.clone()));
    }

    let target_record = records.get(prototype).copied();
    let nested = target_record.map_or(Ok(Prototype::At(Transform::identity())), |target| {
        linked_prototype_transform(ctx, target, records, placements, links)
    });
    let nested = match nested? {
        Prototype::At(nested) => nested,
        unknown @ Prototype::Unknown(_) => return Ok(unknown),
    };
    if let Some(owner) = placements.dropped_elements.get(prototype) {
        return Ok(Prototype::Unknown(owner.clone()));
    }
    if placements.unplaced.contains(prototype) {
        return Ok(Prototype::Unknown(prototype.to_owned()));
    }
    let placement = target_record
        .and_then(ProductNodeRecord::local_transform)
        .map(FiniteFrame::transform)
        .or_else(|| placements.placed.get(prototype).copied())
        .unwrap_or_default();
    match placement.compose(nested) {
        Ok(transform) => Ok(Prototype::At(transform)),
        Err(error) => {
            if links.charged.insert(record.object.clone()) {
                links.deferred.push(Deferred {
                    object: record.object.clone(),
                    record: GivenUpRecord::new(
                        Identity::try_from(record.object.as_str())
                            .map_err(CodecError::malformed)?,
                        Cause::Undecoded,
                        format_args!(
                            "the placement of its linked object {prototype} does not compose: \
                             {error}"
                        ),
                    ),
                });
            }
            Ok(Prototype::Unknown(record.object.clone()))
        }
    }
}

/// The id of the occurrence `child` of object `object` in `container`, a
/// container other than the first that lists the object.
fn listed_occurrence_id(
    object: &str,
    child: &str,
    container: Option<&str>,
) -> Result<OccurrenceId, CodecError> {
    let container = container.map_or("", crate::native::id_key);
    OccurrenceId::mint(crate::native::model_id(
        "occurrence",
        object,
        format!("{child}/{container}"),
    ))
    .map_err(CodecError::malformed)
}

/// Whether occurrence `id` places its object in a container other than the
/// first that lists the object.
pub(crate) fn is_listing_copy(id: &str) -> bool {
    id.rsplit_once(':')
        .is_some_and(|(_, child)| child.contains('/'))
}

/// The coordinate-system group of each object, and the link groups that
/// list it.
///
/// `FreeCAD` holds an object in one coordinate-system group: an `App::Part`,
/// an assembly, or a `PartDesign::Body`. Where several list it, the first in
/// document order holds it. An `App::LinkGroup` lists objects that a group
/// also holds, and shows each again at its own placement.
pub(crate) struct ParentMap<'a> {
    coordinate: HashMap<&'a str, &'a str>,
    parts: HashSet<&'a str>,
    link_groups: HashMap<&'a str, Vec<&'a str>>,
    copy_on_change_groups: HashSet<&'a str>,
    member_order: Vec<(&'a str, Vec<&'a str>)>,
    /// Each list property that lists objects another group holds, with each
    /// such object and the group that holds it.
    conflicts: Vec<(&'a PropertyRecord, Vec<(&'a str, &'a str)>)>,
}

impl<'a> ParentMap<'a> {
    pub(crate) fn new(
        records: &'a [ProductNodeRecord],
        objects: &'a [ObjectRecord],
        properties: &'a HashMap<&'a str, Vec<&'a PropertyRecord>>,
    ) -> Self {
        let mut by_object = HashMap::new();
        for record in records {
            by_object.entry(record.object.as_str()).or_insert(record);
        }
        let mut map = Self {
            coordinate: HashMap::new(),
            parts: HashSet::new(),
            link_groups: HashMap::new(),
            copy_on_change_groups: records
                .iter()
                .filter_map(ProductNodeRecord::copy_on_change_group)
                .filter(|target| target.document().is_none())
                .filter_map(|target| target.object())
                .collect(),
            member_order: Vec::new(),
            conflicts: Vec::new(),
        };
        let mut in_document_order = objects.iter().collect::<Vec<_>>();
        in_document_order.sort_by_key(|object| object.order);
        for object in in_document_order {
            let id = object.id().as_str();
            let owned = properties.get(id).map_or(&[][..], Vec::as_slice);
            let record = by_object.get(id).copied();
            let (carrier, listed): (Option<&'a PropertyRecord>, Vec<&'a str>) =
                match record.map(|record| &record.node) {
                    Some(ProductNode::Part(container)) => {
                        map.parts.insert(id);
                        (
                            owned
                                .iter()
                                .copied()
                                .find(|property| property.name == "Group"),
                            container.members.iter().map(String::as_str).collect(),
                        )
                    }
                    Some(ProductNode::LinkGroup {
                        container,
                        element_objects,
                    }) => {
                        let members = container
                            .members
                            .iter()
                            .chain(element_objects)
                            .map(String::as_str)
                            .collect::<Vec<_>>();
                        for &member in &members {
                            let groups = map.link_groups.entry(member).or_default();
                            if !groups.contains(&id) {
                                groups.push(id);
                            }
                        }
                        map.member_order.push((id, members));
                        continue;
                    }
                    _ if object.type_name == "PartDesign::Body" => {
                        match crate::design::body_membership_property(owned) {
                            Some(group) => (
                                Some(group),
                                group
                                    .links()
                                    .iter()
                                    .flatten()
                                    .filter_map(|link| link.object())
                                    .collect(),
                            ),
                            None => continue,
                        }
                    }
                    _ => continue,
                };
            let mut conflicting = Vec::new();
            for &member in &listed {
                match map.coordinate.get(member).copied() {
                    None => {
                        map.coordinate.insert(member, id);
                    }
                    Some(holder) if holder != id && !conflicting.contains(&(member, holder)) => {
                        conflicting.push((member, holder));
                    }
                    Some(_) => {}
                }
            }
            if map.parts.contains(id) {
                map.member_order.push((id, listed));
            }
            if let Some(carrier) = carrier.filter(|_| !conflicting.is_empty()) {
                map.conflicts.push((carrier, conflicting));
            }
        }
        map
    }

    /// The coordinate-system group that holds `object`.
    pub(crate) fn coordinate(&self, object: &str) -> Option<&'a str> {
        self.coordinate.get(object).copied()
    }

    fn has_product_placement(&self, object: &str) -> bool {
        self.coordinate(object)
            .is_none_or(|holder| self.parts.contains(holder))
            || self.link_groups.contains_key(object)
    }

    /// The product containers that place `object`, first its `App::Part` or
    /// assembly, then each link group that lists it; `[None]` for the
    /// document root. A Body-held object has no root placement.
    /// A copy-on-change group holds a link's copies hidden,
    /// so it places an object only where no other container lists it.
    pub(crate) fn containers(&self, object: &str) -> Vec<Option<&'a str>> {
        let mut found = self
            .coordinate(object)
            .filter(|holder| self.parts.contains(holder))
            .into_iter()
            .collect::<Vec<_>>();
        let groups = self.link_groups.get(object).map_or(&[][..], Vec::as_slice);
        found.extend(
            groups
                .iter()
                .copied()
                .filter(|group| !self.copy_on_change_groups.contains(group)),
        );
        if found.is_empty() {
            found.extend(groups.first().copied());
        }
        if found.is_empty() && self.has_product_placement(object) {
            return vec![None];
        }
        found.into_iter().map(Some).collect()
    }
}

fn link_scale_transform([x, y, z]: [f64; 3]) -> Result<Transform, CodecError> {
    Transform::affine([[x, 0.0, 0.0, 0.0], [0.0, y, 0.0, 0.0], [0.0, 0.0, z, 0.0]])
        .ok_or_else(|| malformed("link scale must be finite"))
}

fn listed_link_element_objects<'a>(
    records: &'a [ProductNodeRecord],
    objects: &[ObjectRecord],
) -> HashSet<&'a str> {
    let objects_by_id = objects
        .iter()
        .map(|object| (object.id().as_str(), object))
        .collect::<HashMap<_, _>>();
    let mut listed_elements = HashSet::new();
    for record in records.iter().filter(|record| {
        matches!(record.node, ProductNode::Occurrence(_))
            && objects_by_id
                .get(record.object.as_str())
                .is_some_and(|object| object.type_name == "App::Link")
    }) {
        for name in record.element_objects() {
            let Some(object) = objects_by_id.get(name.as_str()) else {
                continue;
            };
            if object.type_name != "App::LinkElement" {
                continue;
            }
            listed_elements.insert(name.as_str());
        }
    }
    listed_elements
}

/// Whether link `record` shows its array elements as objects: its stored
/// `ShowElement`, `FreeCAD`'s default of true where it stores none, and,
/// where it stores one the decoder cannot read, whether it lists element
/// objects, which `FreeCAD` keeps only while it shows them.
fn show_element(record: &ProductNodeRecord, properties: &[&PropertyRecord]) -> bool {
    match stored(properties, "ShowElement", |property| {
        bool_value(property, "ShowElement")
    }) {
        Stored::Absent => true,
        Stored::Read(value) => value,
        Stored::Unreadable(..) => !record.element_objects().is_empty(),
    }
}

fn restored_cardinality(
    record: &ProductNodeRecord,
    properties: &[&PropertyRecord],
) -> LinkArrayCardinality {
    if !show_element(record, properties) {
        return record
            .element_cardinality()
            .unwrap_or(LinkArrayCardinality::Scalar);
    }
    NonZeroU64::new(cadmpeg_core::decode::u64_from_index(
        record.element_objects().len(),
    ))
    .map_or(LinkArrayCardinality::Scalar, LinkArrayCardinality::Elements)
}

fn occurrence_count(
    record: &ProductNodeRecord,
    cardinality: LinkArrayCardinality,
) -> Result<NonZeroUsize, CodecError> {
    let elements = match cardinality {
        LinkArrayCardinality::Scalar => return Ok(NonZeroUsize::MIN),
        LinkArrayCardinality::Elements(elements) => elements,
    };
    let count = NonZeroUsize::try_from(elements).map_err(|_| {
        CodecError::malformed(format_args!(
            "{} element count exceeds addressable size",
            record.id
        ))
    })?;
    if count.get() > MAX_LINK_ARRAY_ELEMENTS || u32::try_from(count.get()).is_err() {
        return Err(CodecError::malformed(format_args!(
            "{} link-array element count {count} exceeds {MAX_LINK_ARRAY_ELEMENTS}",
            record.id
        )));
    }
    Ok(count)
}

/// Link occurrences that [`transfer_neutral`] builds from these records.
///
/// The decoder admits this count against the entity ceiling before it builds
/// any occurrence.
pub(crate) fn link_occurrence_count(
    records: &[ProductNodeRecord],
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Result<u64, CodecError> {
    let mut properties_by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    for property in properties {
        properties_by_owner
            .entry(property.owner.as_str())
            .or_default()
            .push(property);
    }
    let element_objects = listed_link_element_objects(records, objects);
    let parent_map = ParentMap::new(records, objects, &properties_by_owner);
    let mut total = 0_u64;
    for record in records.iter().filter(|record| {
        matches!(record.node, ProductNode::Occurrence(_))
            && !element_objects.contains(record.object.as_str())
    }) {
        let owned = properties_by_owner
            .get(record.object.as_str())
            .map_or(&[][..], Vec::as_slice);
        let cardinality = restored_cardinality(record, owned);
        let count =
            cadmpeg_core::decode::u64_from_index(occurrence_count(record, cardinality)?.get())
                .checked_mul(cadmpeg_core::decode::u64_from_index(
                    parent_map.containers(record.object.as_str()).len(),
                ))
                .ok_or_else(|| {
                    CodecError::malformed("FCStd link occurrence count overflows u64")
                })?;
        total = total
            .checked_add(count)
            .ok_or_else(|| CodecError::malformed("FCStd link occurrence count overflows u64"))?;
    }
    Ok(total)
}

pub(crate) fn external_document_reference(
    value: &str,
    attribute: Option<&str>,
) -> ExternalDocument {
    let value = value.to_owned();
    if attribute.is_some_and(|name| name.eq_ignore_ascii_case("file")) {
        ExternalDocument::path(value)
    } else {
        ExternalDocument::document_id(value)
    }
}

pub(crate) fn multiply(left: [[f64; 4]; 4], right: [[f64; 4]; 4]) -> [[f64; 4]; 4] {
    std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            (0..4)
                .map(|index| left[row][index] * right[index][column])
                .sum()
        })
    })
}

fn parse_placement_list(
    property: &PropertyRecord,
    entries: &BTreeMap<String, View<'_>>,
) -> Result<Vec<FiniteFrame>, CodecError> {
    let Some(view) = side_bytes(
        property,
        "App::PropertyPlacementList",
        "PlacementList",
        entries,
    )?
    else {
        return Ok(Vec::new());
    };
    let positions = list_layout::<7>(view, "PlacementList")?;
    let mut placements = Vec::new();
    for positions in positions {
        let [px, py, pz, qx, qy, qz, qw] = positions.map(read_real);
        let values = [px?, py?, pz?, qx?, qy?, qz?, qw?];
        placements.push(placement_components(&values).ok_or_else(|| {
            CodecError::Malformed("PlacementList contains an invalid placement value".into())
        })?);
    }
    Ok(placements)
}

fn parse_vector_list(
    property: &PropertyRecord,
    entries: &BTreeMap<String, View<'_>>,
) -> Result<Vec<cadmpeg_ir::units::FiniteVector<3>>, CodecError> {
    let Some(view) = side_bytes(property, "App::PropertyVectorList", "VectorList", entries)? else {
        return Ok(Vec::new());
    };
    let positions = list_layout::<3>(view, "ScaleList")?;
    let mut vectors = Vec::new();
    for positions in positions {
        let [x, y, z] = positions.map(read_real);
        vectors.push(
            cadmpeg_ir::units::FiniteVector::new([x?, y?, z?]).ok_or_else(|| {
                malformed("element_scales: scale vector components must be finite")
            })?,
        );
    }
    Ok(vectors)
}

fn side_bytes<'a>(
    property: &PropertyRecord,
    expected_type: &str,
    name: &str,
    entries: &BTreeMap<String, View<'a>>,
) -> Result<Option<View<'a>>, CodecError> {
    require_root(property, expected_type, name, name)?;
    if property.side_entries().len() > 1 {
        return Err(CodecError::malformed(format_args!(
            "product property {} has multiple {name} side entries",
            property.id
        )));
    }
    let Some(entry) = property.side_entries().first() else {
        return Ok(None);
    };
    let Some(view) = entries.get(entry).copied() else {
        return Err(CodecError::malformed(format_args!(
            "{property_id} references missing {entry}",
            property_id = property.id
        )));
    };
    Ok(Some(view))
}

fn single_link<'a>(
    property: &'a PropertyRecord,
    expected_type: &str,
    root: &str,
    name: &str,
) -> Result<Option<&'a crate::native::LinkTarget>, CodecError> {
    require_root(property, expected_type, name, root)?;
    if property.links().len() != 1 {
        return Err(CodecError::malformed(format_args!(
            "product property {} requires one {name} target, found {}",
            property.id,
            property.links().len()
        )));
    }
    Ok(property.links()[0].as_ref())
}

fn link_list<'a>(
    property: &'a PropertyRecord,
    expected_type: &str,
    name: &str,
) -> Result<&'a [Option<crate::native::LinkTarget>], CodecError> {
    require_root(property, expected_type, name, "LinkList")?;
    if property
        .values()
        .iter()
        .skip(1)
        .any(|value| value.tag != "Link")
    {
        return Err(CodecError::malformed(format_args!(
            "product property {} has a non-Link child in {name}",
            property.id
        )));
    }
    Ok(property.links())
}

fn require_root(
    property: &PropertyRecord,
    expected_type: &str,
    name: &str,
    root: &str,
) -> Result<(), CodecError> {
    if property.type_name != expected_type {
        return Err(CodecError::malformed(format_args!(
            "product property {} has runtime type {}, expected {expected_type} for {name}",
            property.id, property.type_name
        )));
    }
    if property.values().first().map(|value| value.tag.as_str()) != Some(root)
        || property
            .values()
            .iter()
            .filter(|value| value.tag == root)
            .count()
            != 1
    {
        return Err(CodecError::malformed(format_args!(
            "product property {} requires one {root} value for {name}",
            property.id
        )));
    }
    Ok(())
}

fn single_value<'a>(
    property: &'a PropertyRecord,
    expected_type: &str,
    name: &str,
    root: &str,
) -> Result<&'a crate::native::ValueRecord, CodecError> {
    require_root(property, expected_type, name, root)?;
    if property.values().len() != 1 {
        return Err(CodecError::malformed(format_args!(
            "product property {} has multiple values for {name}",
            property.id
        )));
    }
    Ok(&property.values()[0])
}

#[derive(Clone, Copy)]
enum RealWidth {
    Single,
    Double,
}

impl RealWidth {
    fn bytes(self) -> usize {
        match self {
            Self::Single => 4,
            Self::Double => 8,
        }
    }
}

#[derive(Clone, Copy)]
struct RealPosition<'a> {
    view: View<'a>,
    offset: usize,
    width: RealWidth,
}

fn list_layout<'a, const N: usize>(
    view: View<'a>,
    name: &str,
) -> Result<impl ExactSizeIterator<Item = [RealPosition<'a>; N]>, CodecError> {
    let len = view.end() - view.start();
    if len < link_array::LEN {
        return Err(CodecError::malformed(format_args!("{name} is truncated")));
    }
    let mut head = view;
    head.seek(view.start())
        .ok_or_else(|| CodecError::malformed(format_args!("{name} header is out of bounds")))?;
    let count = usize::try_from(
        head.req_u32_le()
            .map_err(|error| CodecError::malformed(format_args!("link-array count: {error:?}")))?,
    )
    .map_err(CodecError::malformed)?;
    let encoded_len = |width: RealWidth| {
        count
            .checked_mul(N)
            .and_then(|count| count.checked_mul(width.bytes()))
            .and_then(|bytes| link_array::LEN.checked_add(bytes))
    };
    let width = if encoded_len(RealWidth::Double) == Some(len) {
        RealWidth::Double
    } else if encoded_len(RealWidth::Single) == Some(len) {
        RealWidth::Single
    } else {
        return Err(CodecError::malformed(format_args!(
            "{name} count {count} does not match {len} bytes"
        )));
    };
    Ok((0..count).map(move |index| {
        std::array::from_fn(|component| RealPosition {
            view,
            offset: view.start() + link_array::LEN + (index * N + component) * width.bytes(),
            width,
        })
    }))
}

fn read_real(position: RealPosition<'_>) -> Result<f64, CodecError> {
    let mut cursor = position.view;
    cursor
        .seek(position.offset)
        .ok_or_else(|| CodecError::malformed("real position is out of bounds"))?;
    match position.width {
        RealWidth::Double => cursor.req_f64_le().map_err(|error| {
            CodecError::malformed(format_args!("link-array component: {error:?}"))
        }),
        RealWidth::Single => cursor.req_f32_le().map(f64::from).map_err(|error| {
            CodecError::malformed(format_args!("link-array component: {error:?}"))
        }),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ProductKind {
    Group,
    Part,
    LinkGroup,
    Occurrence,
}

fn product_kind(kind: &str) -> Option<ProductKind> {
    match kind {
        "Assembly::AssemblyObject" | "Assembly::AssemblyLink" | "App::Part" => {
            Some(ProductKind::Part)
        }
        "App::DocumentObjectGroup" => Some(ProductKind::Group),
        "App::LinkGroup" => Some(ProductKind::LinkGroup),
        "App::Link" | "App::LinkElement" => Some(ProductKind::Occurrence),
        _ => None,
    }
}

fn metadata_string(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<String>, CodecError> {
    let Some(property) = properties.iter().find(|property| property.name == name) else {
        return Ok(None);
    };
    if property.type_name != "App::PropertyString" {
        return Ok(None);
    }
    let document = match ctx.parse_xml(property.xml.text(), "FreeCAD XML tree") {
        Ok(tree) => tree,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => {
            return Ok(None);
        }
    };
    let root = document.root_element();
    if !root.has_tag_name("Property") {
        return Ok(None);
    }
    let mut values = root.children().filter(roxmltree::Node::is_element);
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some()
        || !value.has_tag_name("String")
        || value.children().any(|node| node.is_element())
    {
        return Ok(None);
    }
    Ok(value.attribute("value").map(str::to_owned))
}

fn bool_value(property: &PropertyRecord, name: &str) -> Result<bool, CodecError> {
    let value = single_value(property, "App::PropertyBool", name, "Bool")?;
    let Some(value) = value.attributes.get("value") else {
        return Err(CodecError::malformed(format_args!(
            "product property {} has no Bool value",
            property.id
        )));
    };
    parse_bool(value).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "product property {} has an invalid Bool value",
            property.id
        ))
    })
}

/// The `ElementCount` of a link array.
fn element_count(property: &PropertyRecord) -> Result<u64, CodecError> {
    let value = single_value(
        property,
        "App::PropertyIntegerConstraint",
        "ElementCount",
        "Integer",
    )?;
    let Some(value) = value.attributes.get("value") else {
        return Err(CodecError::malformed(format_args!(
            "product property {} has no Integer value",
            property.id
        )));
    };
    let Ok(value) = value.parse::<i64>() else {
        return Err(CodecError::malformed(format_args!(
            "product property {} has an invalid Integer value",
            property.id
        )));
    };
    let count = u64::try_from(value).map_err(|_| malformed("negative ElementCount"))?;
    if usize::try_from(count).map_or(true, |count| count > MAX_LINK_ARRAY_ELEMENTS) {
        return Err(CodecError::malformed(format_args!(
            "{} link-array element count {count} exceeds {MAX_LINK_ARRAY_ELEMENTS}",
            property.id
        )));
    }
    Ok(count)
}

fn copy_on_change_value(property: &PropertyRecord) -> Result<NativeCopyOnChangePolicy, CodecError> {
    let value = single_value(
        property,
        "App::PropertyEnumeration",
        "LinkCopyOnChange",
        "Integer",
    )?;
    let Some(raw) = value.attributes.get("value") else {
        return Err(CodecError::malformed(format_args!(
            "product property {} has no enumeration value",
            property.id
        )));
    };
    NativeCopyOnChangePolicy::from_raw(raw.to_owned()).map_err(|_| {
        CodecError::malformed(format_args!(
            "product property {} has an invalid enumeration Integer value",
            property.id
        ))
    })
}

/// The scale of an object whose properties are `owned`: its `ScaleVector`,
/// or its uniform `Scale` where it stores no `ScaleVector` or, with
/// `restore_uniform`, a uniform one.
fn scale_property<'a>(
    owned: &[&'a PropertyRecord],
    restore_uniform: bool,
) -> Stored<'a, Option<FiniteVector<3>>> {
    let vector = match stored(owned, "ScaleVector", vector_property) {
        Stored::Absent => None,
        Stored::Read(vector) => Some(vector),
        Stored::Unreadable(property, error) => return Stored::Unreadable(property, error),
    };
    let restore_uniform = restore_uniform
        && vector.is_some_and(|vector| {
            let [x, y, z] = vector.get();
            x == y && y == z
        });
    if vector.is_some() && !restore_uniform {
        return Stored::Read(vector);
    }
    match stored(owned, "Scale", |property| {
        let value = single_value(property, "App::PropertyFloat", "Scale", "Float")?;
        let Some(value) = value.attributes.get("value") else {
            return Err(CodecError::malformed(format_args!(
                "product property {} has no Float value",
                property.id
            )));
        };
        parse_finite(value, property, "Scale")
    }) {
        Stored::Absent => Stored::Read(restore_uniform.then_some([FiniteReal::ONE; 3].into())),
        Stored::Read(value) => Stored::Read(Some([value; 3].into())),
        Stored::Unreadable(property, error) => Stored::Unreadable(property, error),
    }
}

fn vector_property(property: &PropertyRecord) -> Result<FiniteVector<3>, CodecError> {
    let value = single_value(
        property,
        "App::PropertyVector",
        "ScaleVector",
        "PropertyVector",
    )?;
    let component = |name: &str| {
        let Some(value) = value.attributes.get(name) else {
            return Err(CodecError::malformed(format_args!(
                "product property {} has no {name} vector component",
                property.id
            )));
        };
        parse_finite(value, property, "ScaleVector")
    };
    Ok([
        component("valueX")?,
        component("valueY")?,
        component("valueZ")?,
    ]
    .into())
}

fn parse_finite(
    value: &str,
    property: &PropertyRecord,
    name: &str,
) -> Result<FiniteReal, CodecError> {
    let Some(value) = value.parse::<f64>().ok().and_then(FiniteReal::new) else {
        return Err(CodecError::malformed(format_args!(
            "product property {} has an invalid finite value for {name}",
            property.id
        )));
    };
    Ok(value)
}

fn bool_list(property: &PropertyRecord, name: &str) -> Result<Vec<bool>, CodecError> {
    let value = single_value(property, "App::PropertyBoolList", name, "BoolList")?;
    let Some(encoded) = value.attributes.get("value") else {
        return Err(CodecError::malformed(format_args!(
            "product property {} has no BoolList value",
            property.id
        )));
    };
    if encoded.bytes().any(|byte| !matches!(byte, b'0' | b'1')) {
        return Err(CodecError::malformed(format_args!(
            "product property {} has an invalid BoolList bit string",
            property.id
        )));
    }
    // FreeCAD writes the most-significant bit first: the rightmost source bit
    // belongs to element zero. The raw XML remains on the property record;
    // this projection follows the element order used by the other carriers.
    let mut values = Vec::new();
    values.extend(encoded.bytes().rev().map(|byte| byte == b'1'));
    Ok(values)
}

pub(crate) fn product_cycle_nodes<'a>(
    nodes: &HashMap<&'a str, &'a ProductNodeRecord>,
) -> HashSet<&'a str> {
    let given_up = followed_link_cycle_nodes(nodes);
    cycle_nodes(nodes, |node| {
        node.members()
            .iter()
            .map(String::as_str)
            .chain(node.prototype().filter(|_| {
                node.external_document().is_none() && !given_up.contains(node.object.as_str())
            }))
            .chain(node.element_objects().iter().map(String::as_str))
            .collect()
    })
}

/// The links on a cycle of links that follow their linked objects, which
/// [`transfer_neutral`] gives up.
fn followed_link_cycle_nodes<'a>(
    nodes: &HashMap<&'a str, &'a ProductNodeRecord>,
) -> HashSet<&'a str> {
    cycle_nodes(nodes, |node| {
        node.prototype()
            .filter(|_| node.link_transform() == Some(true) && node.external_document().is_none())
            .into_iter()
            .collect()
    })
}

/// The records of `nodes` on a cycle of the edges `targets` gives.
fn cycle_nodes<'a>(
    nodes: &HashMap<&'a str, &'a ProductNodeRecord>,
    targets: impl Fn(&'a ProductNodeRecord) -> Vec<&'a str>,
) -> HashSet<&'a str> {
    let edges = |name: &'a str| {
        nodes
            .get(name)
            .map(|node| targets(node))
            .unwrap_or_default()
            .into_iter()
            .filter(|target| nodes.contains_key(target))
    };
    let mut reverse = HashMap::<&str, Vec<&str>>::new();
    for &source in nodes.keys() {
        reverse.insert(source, Vec::new());
    }
    for &source in nodes.keys() {
        for target in edges(source) {
            if let Some(sources) = reverse.get_mut(target) {
                sources.push(source);
            }
        }
    }

    let mut visited = HashSet::new();
    let mut finish = Vec::new();
    for &root in nodes.keys() {
        if !visited.insert(root) {
            continue;
        }
        let mut stack = Vec::new();
        stack.push((root, edges(root).collect::<Vec<_>>(), 0_usize));
        while let Some((current, targets, next)) = stack.last_mut() {
            if let Some(&target) = targets.get(*next) {
                *next += 1;
                if visited.insert(target) {
                    stack.push((target, edges(target).collect(), 0));
                }
            } else {
                finish.push(*current);
                stack.pop();
            }
        }
    }

    let mut assigned = HashSet::new();
    let mut cyclic = HashSet::new();
    while let Some(root) = finish.pop() {
        if !assigned.insert(root) {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = Vec::new();
        stack.push(root);
        while let Some(current) = stack.pop() {
            component.push(current);
            for &source in reverse.get(current).into_iter().flatten() {
                if assigned.insert(source) {
                    stack.push(source);
                }
            }
        }
        let self_cycle = component.len() == 1 && edges(component[0]).any(|target| target == root);
        if component.len() > 1 || self_cycle {
            for member in component {
                cyclic.insert(member);
            }
        }
    }
    cyclic
}

#[cfg(test)]
pub(crate) mod tests;
