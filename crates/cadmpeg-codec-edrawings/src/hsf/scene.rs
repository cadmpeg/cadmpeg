// SPDX-License-Identifier: Apache-2.0
//! The scene: records of the model stream collected into library bodies,
//! tree nodes, and the object store.
//!
//! The walker yields records in stream order. The scene keeps a segment
//! stack, counts the object slots `q` (the k-th slot refers to object id k),
//! and sorts what it sees into:
//!
//! - library bodies: segments named `/include library/.../shared/bodyJ` or
//!   `../.../shared/bodyJ`, with
//!   the slots of every shell placeholder below them and the body colour;
//! - tree nodes: `assempart*`, `instance*`, and `subassem*` segments, with
//!   the matrix and include paths of their `part` child and the slots of
//!   their name text objects;
//! - the object store: shells, shell instances, and text objects by id;
//! - the display state: which tree paths the active state hides.
//!
//! The grammar of the stream decides which colour records count: the body
//! colour record `~` of re-saved streams, or the materials `"` of streams
//! that `SolidWorks` wrote, at body, sub-segment, and node level.

use std::collections::BTreeMap;

use cadmpeg_core::convert::truncate_f64_to_u8;
use cadmpeg_core::decode::View;

use super::walk::{Instance, Record, Shell, SlotKind, WalkError, Walker};
use crate::dialect::Grammar;

/// Most tree nodes one stream may declare.
pub(super) const MAX_NODES: usize = 100_000;
/// Most library bodies one stream may declare.
pub(super) const MAX_BODIES: usize = 100_000;
/// Most object slots one stream may declare.
pub(super) const MAX_SLOTS: usize = 8_000_000;
/// Most objects the store keeps.
pub(super) const MAX_OBJECTS: usize = 8_000_000;
/// Most display state components the scene keeps.
pub(super) const MAX_COMPONENTS: usize = 200_000;

/// Prefix of a library segment name.
const LIBRARY_PREFIX: &str = "/include library/";
/// Prefix of a library segment name written as a relative path.
const RELATIVE_PREFIX: &str = "../";
/// Infix of a library body segment name.
const BODY_INFIX: &str = "/shared/body";
/// Name prefixes of tree nodes.
const NODE_PREFIXES: [&str; 3] = ["assempart", "instance", "subassem"];
/// Name of the segment that holds a node's geometry and matrix.
const PART_SEGMENT: &str = "part";
/// Name of the segment that holds a node's name slots.
const REFERENCE_SEGMENT: &str = "reference";
/// Name of the display name slot segment.
const REFNAME_SEGMENT: &str = "refname";
/// Name of the second name slot segment.
const NAME_SEGMENT: &str = "name";
/// Name of the display state container.
const DISPLAY_STATES_SEGMENT: &str = "displaystates";

/// A library body: its path, colour, and shell placeholder slots.
#[derive(Debug, Default)]
pub(super) struct LibraryBody {
    /// The segment path, as written.
    pub(super) path: String,
    /// The body colour.
    pub(super) colour: Option<[u8; 3]>,
    /// The materials of the sub-segments, in stream order: the colour of
    /// each, `None` when the description holds no colour.
    pub(super) sub_colours: Vec<Option<[u8; 3]>>,
    /// Object ids of the shell placeholders, in stream order.
    pub(super) slots: Vec<u32>,
}

/// A tree node.
#[derive(Debug, Default)]
pub(super) struct Node {
    /// The segment name.
    pub(super) segment: String,
    /// Index of the enclosing node.
    pub(super) parent: Option<usize>,
    /// Whether the node has a `part` child: a placed instance.
    pub(super) is_instance: bool,
    /// The modelling matrix, metres, row-vector convention.
    pub(super) matrix: Option<[f32; 12]>,
    /// The include paths of the instance bodies, in stream order.
    pub(super) body_paths: Vec<String>,
    /// Object id of the `refname` text.
    pub(super) refname_slot: Option<u32>,
    /// Object id of the `name` text.
    pub(super) name_slot: Option<u32>,
    /// The `name` of the node's user option.
    pub(super) user_name: Option<String>,
    /// The colour of the material that the node itself carries.
    pub(super) colour: Option<[u8; 3]>,
    /// The `defaultcolor` of the node's user option.
    pub(super) default_colour: Option<[u8; 3]>,
    /// Whether the node's user option sets `defaultvisibility=off`.
    pub(super) hidden_by_default: bool,
}

/// An object of the store.
#[derive(Debug, Clone, Copy)]
pub(super) enum Object<'a> {
    /// A shell.
    Shell(Shell<'a>),
    /// A shell instance.
    Instance(Instance),
}

/// One component of a display state.
#[derive(Debug)]
pub(super) struct Component {
    /// The display state it belongs to.
    pub(super) state: String,
    /// The tree path it names.
    pub(super) path: String,
    /// Whether the node is visible.
    pub(super) visible: bool,
    /// Whether the component overrides the colour or the transmission.
    pub(super) overrides_appearance: bool,
}

/// What a segment on the stack is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    /// The outermost library body segment (index into the bodies).
    Body(usize),
    /// A tree node (index into the nodes).
    Node(usize),
    /// A `part` segment.
    Part,
    /// A `reference` segment.
    Reference,
    /// A `refname` segment.
    RefName,
    /// A `name` segment.
    NameSlot,
    /// The `displaystates` segment.
    DisplayStates,
    /// A display state segment.
    State(usize),
    /// A component of a display state.
    Component(usize),
    /// Any other segment.
    Other,
    /// The scope of a side object.
    Side,
}

/// Everything the model stream holds that the emitter needs.
#[derive(Debug, Default)]
pub(super) struct Scene<'a> {
    /// The record rules the stream was read with.
    pub(super) grammar: Option<Grammar>,
    /// Library bodies in stream order.
    pub(super) bodies: Vec<LibraryBody>,
    /// Tree nodes in stream order.
    pub(super) nodes: Vec<Node>,
    /// Shells and shell instances by object id.
    pub(super) objects: BTreeMap<u32, Object<'a>>,
    /// Text objects by object id.
    pub(super) texts: BTreeMap<u32, String>,
    /// The document name text slot.
    pub(super) document_slot: Option<u32>,
    /// Display state components.
    pub(super) components: Vec<Component>,
    /// Display state names, in stream order.
    states: Vec<String>,
    /// The name of the active display state.
    pub(super) current_state: Option<String>,
    /// The name of the default display state.
    default_state: Option<String>,
    /// Slots of shell placeholders that lie outside every library body.
    pub(super) orphan_slots: usize,
    /// Objects whose id was already taken.
    pub(super) duplicate_objects: usize,
    /// Number of records walked.
    pub(super) records: usize,
}

/// A channel in `0..=1` as a byte.
fn channel_byte(value: f64) -> Option<u8> {
    truncate_f64_to_u8((value * f64::from(u8::MAX)).round())
}

/// The colour of a material description: the numbers after `r=`, `g=`, and
/// `b=` of its diffuse colour, in `0..=1`.
pub(super) fn diffuse_colour(text: &str) -> Option<[u8; 3]> {
    let (_, rest) = text.split_once("diffuse color=")?;
    let mut channels = [None; 3];
    for token in rest.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '`' | ',')) {
        for (slot, key) in channels.iter_mut().zip(["r=", "g=", "b="]) {
            if let Some(number) = token.strip_prefix(key) {
                *slot = number.parse::<f64>().ok();
            }
        }
        if token.starts_with("diffuse") && !token.starts_with("diffuse color") {
            break;
        }
    }
    let [Some(r), Some(g), Some(b)] = channels else {
        return None;
    };
    Some([channel_byte(r)?, channel_byte(g)?, channel_byte(b)?])
}

/// The three channels of an `r g b` option value, bytes.
fn option_colour(value: &str) -> Option<[u8; 3]> {
    let mut channels = value
        .split_whitespace()
        .map(|part| part.parse::<f64>().ok());
    let (r, g, b) = (channels.next()??, channels.next()??, channels.next()??);
    Some([channel_byte(r)?, channel_byte(g)?, channel_byte(b)?])
}

/// Decodes the Latin-1 bytes of a name.
pub(super) fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// Decodes UTF-16LE bytes; a trailing odd byte and bad pairs become U+FFFD.
fn utf16(bytes: &[u8]) -> String {
    let mut view = View::over_retained(bytes);
    let units = std::iter::from_fn(|| view.u16_le());
    char::decode_utf16(units)
        .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

/// The `key=value` fields of a user option string. Values may be quoted with
/// backticks, which may contain commas.
pub(super) fn option_fields(text: &str) -> Vec<(&str, &str)> {
    let mut fields = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let Some((key, after)) = rest.split_once('=') else {
            break;
        };
        let (value, remainder) = if let Some(quoted) = after.strip_prefix('`') {
            match quoted.split_once('`') {
                Some((value, tail)) => (value, tail.strip_prefix(',').unwrap_or(tail)),
                None => (quoted, ""),
            }
        } else {
            match after.split_once(',') {
                Some((value, tail)) => (value, tail),
                None => (after, ""),
            }
        };
        fields.push((key.trim(), value));
        rest = remainder;
    }
    fields
}

impl<'a> Scene<'a> {
    /// Walks `view` and collects its scene.
    pub(super) fn collect(view: View<'a>, grammar: Grammar) -> Result<Self, WalkError> {
        let mut builder = Builder {
            grammar,
            scene: Scene {
                grammar: Some(grammar),
                ..Scene::default()
            },
            ..Builder::default()
        };
        let mut walker = Walker::new(view, grammar);
        loop {
            let record = walker.next()?;
            if matches!(record, Record::End) {
                break;
            }
            builder.record(record).map_err(|reason| WalkError {
                offset: walker.offset(),
                reason,
            })?;
        }
        Ok(builder.finish())
    }
}

struct Builder<'a> {
    grammar: Grammar,
    scene: Scene<'a>,
    stack: Vec<Frame>,
    next_slot: u32,
    /// The segment closed by the previous record, when that record was a close.
    closed: Option<Frame>,
}

impl Default for Builder<'_> {
    fn default() -> Self {
        Self {
            grammar: Grammar::Resaved,
            scene: Scene::default(),
            stack: Vec::new(),
            next_slot: 0,
            closed: None,
        }
    }
}

impl<'a> Builder<'a> {
    fn finish(mut self) -> Scene<'a> {
        if self.grammar.solidworks_layout() && self.scene.current_state.is_none() {
            self.scene.current_state = self.scene.default_state.take();
        }
        self.scene
    }

    fn top(&self) -> Option<Frame> {
        self.stack.last().copied()
    }

    fn enclosing_node(&self) -> Option<usize> {
        self.stack.iter().rev().find_map(|frame| match frame {
            Frame::Node(index) => Some(*index),
            _ => None,
        })
    }

    fn enclosing_body(&self) -> Option<usize> {
        self.stack.iter().find_map(|frame| match frame {
            Frame::Body(index) => Some(*index),
            _ => None,
        })
    }

    fn open(&mut self, name: &str) -> Result<(), &'static str> {
        let under = self.top();
        let frame = if (name.starts_with(LIBRARY_PREFIX) || name.starts_with(RELATIVE_PREFIX))
            && name.contains(BODY_INFIX)
            && self.enclosing_body().is_none()
        {
            let index = match self.scene.bodies.iter().position(|body| body.path == name) {
                Some(index) => index,
                None => {
                    if self.scene.bodies.len() >= MAX_BODIES {
                        return Err("library body count exceeds the cap");
                    }
                    self.scene.bodies.push(LibraryBody {
                        path: name.to_owned(),
                        ..LibraryBody::default()
                    });
                    self.scene.bodies.len() - 1
                }
            };
            Frame::Body(index)
        } else if NODE_PREFIXES.iter().any(|prefix| name.starts_with(prefix)) {
            if self.scene.nodes.len() >= MAX_NODES {
                return Err("tree node count exceeds the cap");
            }
            let parent = self.enclosing_node();
            self.scene.nodes.push(Node {
                segment: name.to_owned(),
                parent,
                ..Node::default()
            });
            Frame::Node(self.scene.nodes.len() - 1)
        } else if name == PART_SEGMENT && matches!(under, Some(Frame::Node(_))) {
            if let Some(Frame::Node(index)) = under {
                if let Some(node) = self.scene.nodes.get_mut(index) {
                    node.is_instance = true;
                }
            }
            Frame::Part
        } else if name == REFERENCE_SEGMENT {
            Frame::Reference
        } else if name == REFNAME_SEGMENT && under == Some(Frame::Reference) {
            Frame::RefName
        } else if name == NAME_SEGMENT && matches!(under, Some(Frame::Node(_))) {
            Frame::NameSlot
        } else if name == DISPLAY_STATES_SEGMENT {
            Frame::DisplayStates
        } else if under == Some(Frame::DisplayStates) {
            self.scene.states.push(name.to_owned());
            Frame::State(self.scene.states.len() - 1)
        } else if let (Some(Frame::State(state)), true) = (under, name != NAME_SEGMENT) {
            let state_name = self.scene.states.get(state).cloned().unwrap_or_default();
            if self.scene.components.len() >= MAX_COMPONENTS {
                return Err("display component count exceeds the cap");
            }
            self.scene.components.push(Component {
                state: state_name,
                path: String::new(),
                visible: true,
                overrides_appearance: false,
            });
            Frame::Component(self.scene.components.len() - 1)
        } else {
            Frame::Other
        };
        self.stack.push(frame);
        Ok(())
    }

    fn user_option(&mut self, text: &str) {
        match self.top() {
            Some(Frame::Node(index)) => {
                let fields = option_fields(text);
                let solidworks = self.grammar.solidworks_layout();
                if let Some(node) = self.scene.nodes.get_mut(index) {
                    for (key, value) in fields {
                        match key {
                            "name" => node.user_name = Some(value.to_owned()),
                            "defaultvisibility" if solidworks => {
                                node.hidden_by_default = value == "off";
                            }
                            "defaultcolor" if solidworks => {
                                node.default_colour = option_colour(value);
                            }
                            _ => {}
                        }
                    }
                }
            }
            Some(Frame::DisplayStates) => {
                for (key, value) in option_fields(text) {
                    match key {
                        "current" => self.scene.current_state = Some(value.to_owned()),
                        "default" if self.grammar.solidworks_layout() => {
                            self.scene.default_state = Some(value.to_owned());
                        }
                        _ => {}
                    }
                }
            }
            Some(Frame::Component(index)) => {
                let fields = option_fields(text);
                if let Some(component) = self.scene.components.get_mut(index) {
                    for (key, value) in fields {
                        match key {
                            "path" => value.clone_into(&mut component.path),
                            "vis" => component.visible = value != "off",
                            "color" | "transmission" => component.overrides_appearance = true,
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn slot(&mut self, kind: SlotKind) -> Result<(), &'static str> {
        let id = self.next_slot;
        if usize::try_from(id).map_or(true, |count| count >= MAX_SLOTS) {
            return Err("object slot count exceeds the cap");
        }
        self.next_slot = id + 1;
        match kind {
            SlotKind::Shell => match self.enclosing_body() {
                Some(index) => {
                    if let Some(body) = self.scene.bodies.get_mut(index) {
                        body.slots.push(id);
                    }
                }
                None => self.scene.orphan_slots += 1,
            },
            SlotKind::Text => match self.closed {
                Some(Frame::RefName) => match self.enclosing_node() {
                    Some(index) => {
                        if let Some(node) = self.scene.nodes.get_mut(index) {
                            node.refname_slot = node.refname_slot.or(Some(id));
                        }
                    }
                    None => self.scene.document_slot = self.scene.document_slot.or(Some(id)),
                },
                Some(Frame::NameSlot) => {
                    if let Some(node) = self
                        .enclosing_node()
                        .and_then(|index| self.scene.nodes.get_mut(index))
                    {
                        node.name_slot = node.name_slot.or(Some(id));
                    }
                }
                _ => {}
            },
            SlotKind::Texture | SlotKind::Other => {}
        }
        Ok(())
    }

    /// Files the colour of a material under the segment that holds it.
    fn material(&mut self, colour: Option<[u8; 3]>) {
        let parent = self
            .stack
            .len()
            .checked_sub(2)
            .and_then(|index| self.stack.get(index))
            .copied();
        match (self.top(), parent) {
            (Some(Frame::Body(index)), _) => {
                if let Some(body) = self.scene.bodies.get_mut(index) {
                    body.colour = body.colour.or(colour);
                }
            }
            (Some(Frame::Node(index)), _) => {
                if let Some(node) = self.scene.nodes.get_mut(index) {
                    node.colour = node.colour.or(colour);
                }
            }
            (Some(Frame::Other), Some(Frame::Body(index))) => {
                if let Some(body) = self.scene.bodies.get_mut(index) {
                    body.sub_colours.push(colour);
                }
            }
            _ => {}
        }
    }

    fn object(&mut self, id: u32, object: Object<'a>) -> Result<(), &'static str> {
        if self.scene.objects.len() >= MAX_OBJECTS {
            return Err("object count exceeds the cap");
        }
        if self.scene.objects.contains_key(&id) || self.scene.texts.contains_key(&id) {
            self.scene.duplicate_objects += 1;
        } else {
            self.scene.objects.insert(id, object);
        }
        Ok(())
    }

    fn record(&mut self, record: Record<'a>) -> Result<(), &'static str> {
        self.scene.records += 1;
        let closed = self.closed.take();
        match record {
            Record::Open(name) => self.open(&latin1(name))?,
            Record::Close => self.closed = self.stack.pop(),
            Record::UserOption(text) => self.user_option(&latin1(text)),
            Record::UserOptionWide(text) => self.user_option(&utf16(text)),
            Record::Colour(rgb) => {
                if let Some(Frame::Body(index)) = self.top() {
                    if let Some(body) = self.scene.bodies.get_mut(index) {
                        body.colour = body.colour.or(Some(rgb));
                    }
                }
            }
            Record::Matrix(matrix) => {
                if matches!(self.top(), Some(Frame::Part | Frame::Node(_))) {
                    if let Some(node) = self
                        .enclosing_node()
                        .and_then(|index| self.scene.nodes.get_mut(index))
                    {
                        node.matrix = node.matrix.or(Some(matrix));
                    }
                }
            }
            Record::Include(path) => {
                if self.top() == Some(Frame::Part) {
                    if let Some(node) = self
                        .enclosing_node()
                        .and_then(|index| self.scene.nodes.get_mut(index))
                    {
                        node.body_paths.push(latin1(path));
                    }
                }
            }
            Record::Slot(kind) => {
                self.closed = closed;
                self.slot(kind)?;
            }
            Record::Shell(shell) => self.object(shell.id, Object::Shell(shell))?,
            Record::Instance(instance) => match u32::try_from(instance.own) {
                Ok(id) => self.object(id, Object::Instance(instance))?,
                Err(_) => return Err("shell instance id exceeds 32 bits"),
            },
            Record::SideEmpty | Record::SideImage | Record::SideGeometry => {
                self.stack.push(Frame::Side);
            }
            Record::SideText(id, text) => {
                self.stack.push(Frame::Side);
                if self.scene.objects.contains_key(&id) || self.scene.texts.contains_key(&id) {
                    self.scene.duplicate_objects += 1;
                } else if self.scene.texts.len() < MAX_OBJECTS {
                    self.scene.texts.insert(id, utf16(text));
                } else {
                    return Err("text object count exceeds the cap");
                }
            }
            Record::Material(text) => {
                if self.grammar.solidworks_layout() {
                    self.material(diffuse_colour(&latin1(text)));
                }
            }
            Record::Pad => self.closed = closed,
            Record::Header
            | Record::Trailer
            | Record::Camera
            | Record::LineStyles
            | Record::Separator
            | Record::Ignored
            | Record::ShellPlaceholder
            | Record::End => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
