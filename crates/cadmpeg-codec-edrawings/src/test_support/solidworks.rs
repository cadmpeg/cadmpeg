// SPDX-License-Identifier: Apache-2.0
//! A writer for the inflated model stream of the `SolidWorks` grammar, built
//! from the layouts in docs/formats/edrawings.md section 3.
//!
//! [`SolidWorksModel`] describes a library of bodies and a tree of
//! sub-assemblies and parts, and writes the whole stream: tree with its body
//! definitions, display states, line styles, object store (in reverse id
//! order), side objects, trailer, and end byte. A part lists several bodies,
//! the first part that names a body defines it, and the library path of a body
//! holds no `model_K_include` level.

use super::hsf::{Item, Stream};
use crate::dialect::Grammar;

/// The library path of body `index`.
pub(crate) fn body_path(index: usize) -> String {
    format!("/include library/shared/body{index}")
}

/// A shell of a body: a plain item, or an instance of a shell in another body.
#[derive(Debug, Clone)]
pub(crate) enum SwItem {
    /// An item of the same kind as in the re-saved grammar.
    Local(Item),
    /// An instance of a shell that lives in another body.
    Foreign {
        /// Index of the body that holds the source shell. The body must be
        /// defined earlier in the stream.
        body: usize,
        /// Index of the source item within that body (over all sub-segments).
        source: usize,
        /// The affine.
        matrix: [f32; 12],
    },
}

/// A sub-segment: a colour group of shells.
#[derive(Debug, Clone, Default)]
pub(crate) struct SwSub {
    /// The shells.
    pub(crate) items: Vec<SwItem>,
    /// The diffuse colour of the sub-segment material, `0..=1`.
    pub(crate) colour: Option<[f32; 3]>,
}

/// A library body.
#[derive(Debug, Clone, Default)]
pub(crate) struct SwBody {
    /// The diffuse colour of the body material, `0..=1`.
    pub(crate) colour: Option<[f32; 3]>,
    /// The sub-segments.
    pub(crate) subs: Vec<SwSub>,
}

/// A tree node.
#[derive(Debug, Clone)]
pub(crate) enum SwNode {
    /// A part placed in the assembly.
    Leaf {
        /// Segment name, such as `assempart7`.
        segment: String,
        /// Display name.
        name: String,
        /// Library bodies, in include order.
        bodies: Vec<usize>,
        /// The absolute modelling matrix.
        matrix: [f32; 12],
        /// A part-level colour override, `0..=1`.
        colour: Option<[f32; 3]>,
        /// Whether the part sets `defaultvisibility=off`.
        hidden_by_default: bool,
    },
    /// A sub-assembly.
    Sub {
        /// Segment name, such as `subassem3`.
        segment: String,
        /// Display name.
        name: String,
        /// Children.
        children: Vec<SwNode>,
    },
}

/// A display state: its name and the `(segment, visible)` pairs it lists.
#[derive(Debug, Clone)]
pub(crate) struct SwState {
    /// Segment name, such as `disp0`.
    pub(crate) name: String,
    /// Listed nodes.
    pub(crate) components: Vec<(String, bool)>,
}

/// A whole `SolidWorks` model.
#[derive(Debug, Clone, Default)]
pub(crate) struct SolidWorksModel {
    /// Document name.
    pub(crate) document: String,
    /// Library bodies.
    pub(crate) bodies: Vec<SwBody>,
    /// Top-level nodes.
    pub(crate) tree: Vec<SwNode>,
    /// Display states.
    pub(crate) states: Vec<SwState>,
    /// The name of the default display state.
    pub(crate) default_state: Option<String>,
}

/// The tree path prefix of the display state components.
pub(crate) const TREE_PATH: &str = "config0/sheet0/view0/3dmodel/includes";

fn rgb_text(rgb: [f32; 3]) -> String {
    let [r, g, b] = rgb;
    format!("{r:.6} {g:.6} {b:.6}")
}

struct Writer<'a> {
    spec: &'a SolidWorksModel,
    out: Stream,
    next_id: u32,
    store: Vec<(u32, Vec<u8>)>,
    defined: Vec<bool>,
    /// Object ids of the items of each defined body.
    ids: Vec<Vec<u32>>,
}

impl Writer<'_> {
    fn text_slot(&mut self, segment: &str, text: &str) {
        self.out.open(segment).v_text().close().slot();
        let id = self.next_id;
        self.next_id += 1;
        let mut object = Stream::of(Grammar::SolidWorks);
        object.side_text(id, text);
        self.store.push((id, object.into_bytes()));
    }

    fn reference(&mut self, name: &str) {
        self.out.open("reference");
        self.text_slot("refname", name);
        self.text_slot("path", "C:\\private\\source.sldprt");
        self.out.close();
    }

    fn define(&mut self, index: usize) {
        let Some(body) = self.spec.bodies.get(index) else {
            return;
        };
        self.out.open(&body_path(index));
        let mut ids: Vec<u32> = Vec::new();
        for (sub, group) in body.subs.iter().enumerate() {
            self.out.open(&format!("body{index}sub{sub}"));
            for item in &group.items {
                let id = self.next_id;
                self.next_id += 1;
                self.out.shell_placeholder().slot();
                let mut object = Stream::of(Grammar::SolidWorks);
                match item {
                    SwItem::Local(Item::Shell { points, strips }) => {
                        object.shell(id, points, strips);
                    }
                    SwItem::Local(Item::Instance { source, matrix }) => {
                        let source_id = ids.get(*source).copied().unwrap_or(0);
                        object.instance(u64::from(source_id), u64::from(id), matrix);
                    }
                    SwItem::Foreign {
                        body,
                        source,
                        matrix,
                    } => {
                        let source_id = self
                            .ids
                            .get(*body)
                            .and_then(|ids| ids.get(*source))
                            .copied()
                            .unwrap_or(0);
                        object.instance(u64::from(source_id), u64::from(id), matrix);
                    }
                }
                ids.push(id);
                self.store.push((id, object.into_bytes()));
            }
            if let Some(rgb) = group.colour {
                self.out.solidworks_material(rgb);
            }
            self.out.close();
        }
        if let Some(rgb) = body.colour {
            self.out.solidworks_material(rgb);
        }
        self.out.close();
        if let Some(slot) = self.ids.get_mut(index) {
            *slot = ids;
        }
    }

    fn node(&mut self, node: &SwNode) {
        match node {
            SwNode::Leaf {
                segment,
                name,
                bodies,
                matrix,
                colour,
                hidden_by_default,
            } => {
                self.out.open(segment);
                self.reference(name);
                self.out.open("part");
                for &body in bodies {
                    let first = !self.defined.get(body).copied().unwrap_or(true);
                    if first {
                        if let Some(flag) = self.defined.get_mut(body) {
                            *flag = true;
                        }
                        self.define(body);
                    }
                    self.out.include(&body_path(body));
                }
                self.out.modelling_matrix(matrix).close();
                self.text_slot("name", name);
                self.out.solidworks_h_name();
                if *hidden_by_default {
                    self.out.user_option("defaultvisibility=off").v_text();
                }
                if let Some(rgb) = colour {
                    self.out
                        .user_option(&format!(
                            "defaultcolor={},defaulttransmission=0",
                            rgb_text(*rgb)
                        ))
                        .solidworks_material(*rgb)
                        .solidworks_r_run(36);
                }
                self.out.close();
            }
            SwNode::Sub {
                segment,
                name,
                children,
            } => {
                self.out.open(segment);
                self.reference(name);
                self.text_slot("name", name);
                for child in children {
                    self.node(child);
                }
                self.out.close();
            }
        }
    }

    fn states(&mut self) {
        let mut options = String::new();
        if let Some(default) = &self.spec.default_state {
            options = format!("default={default},default_linked={default}");
        }
        self.out.open("displaystates").user_option(&options);
        for state in &self.spec.states {
            self.out.open(&state.name);
            self.text_slot("name", &format!("State {}", state.name));
            for (index, (segment, visible)) in state.components.iter().enumerate() {
                let vis = if *visible { "on" } else { "off" };
                self.out
                    .open(&format!("comp{index}"))
                    .user_option(&format!("path={TREE_PATH}/{segment},vis={vis}"))
                    .close();
            }
            self.out.close();
        }
        self.out.close();
    }
}

impl SolidWorksModel {
    /// The inflated model stream.
    pub(crate) fn stream(&self) -> Vec<u8> {
        let mut writer = Writer {
            spec: self,
            out: Stream::of(Grammar::SolidWorks),
            next_id: 0,
            store: Vec::new(),
            defined: vec![false; self.bodies.len()],
            ids: vec![Vec::new(); self.bodies.len()],
        };
        writer.out.header(2).camera("default");
        writer.reference(&self.document);
        writer.out.open("config0").open("sheet0").open("view0");
        writer.out.open("3dmodel").open("includes");
        writer.out.h_attribute();
        for node in &self.tree {
            writer.node(node);
        }
        writer.out.close().close().close().close();
        if !self.states.is_empty() {
            writer.states();
        }
        writer.out.close();
        writer.out.line_styles();
        writer
            .out
            .user_option("published_link_disp_states_to_configs=off");
        writer.out.raw(&[b'h', 1]);
        writer.out.separator();
        writer.out.open("?Include Library/references");
        let mut store = std::mem::take(&mut writer.store);
        store.sort_by_key(|(id, _)| std::cmp::Reverse(*id));
        let (texts, geometry): (Vec<_>, Vec<_>) = store
            .into_iter()
            .partition(|(_, bytes)| bytes.first() == Some(&b's'));
        for (_, bytes) in &geometry {
            writer.out.raw(bytes);
        }
        writer.out.close();
        writer.out.separator();
        for (_, bytes) in &texts {
            writer.out.raw(bytes).close();
        }
        writer.out.separator().separator();
        writer.out.trailer_and_end();
        writer.out.into_bytes()
    }
}
