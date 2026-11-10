// SPDX-License-Identifier: Apache-2.0
//! A writer for the inflated model stream, built from the layouts in
//! docs/formats/edrawings.md section 4.
//!
//! [`Stream`] appends single records. [`ModelSpec`] describes a library of
//! bodies and an instance tree and writes the whole stream: tree, object
//! store (in reverse id order, as the store is not sorted), side objects,
//! trailer, and end byte.
//!
//! A [`Stream`] writes the records of one [`Grammar`]. The record methods that
//! differ between the grammars write the form of the stream's grammar; the
//! `solidworks_*` methods write the records that only `SolidWorks` streams hold.

use crate::dialect::Grammar;

/// Vertex count above which a face list uses 16-bit indices.
const WIDE_INDEX_FROM: usize = 257;

/// The identity matrix in the stream layout: rows then translation.
pub(crate) const IDENTITY: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];

/// A matrix with rotation rows `rows` and translation `t`, metres.
pub(crate) fn matrix(rows: [[f32; 3]; 3], t: [f32; 3]) -> [f32; 12] {
    let [a, b, c] = rows;
    [
        a[0], a[1], a[2], b[0], b[1], b[2], c[0], c[1], c[2], t[0], t[1], t[2],
    ]
}

/// A translation matrix, metres.
pub(crate) fn translation(t: [f32; 3]) -> [f32; 12] {
    matrix([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t)
}

/// A record writer.
#[derive(Debug, Default, Clone)]
pub(crate) struct Stream {
    bytes: Vec<u8>,
    solidworks: bool,
    short_image: bool,
}

impl Stream {
    /// An empty stream of the re-saved grammar.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// An empty stream of `grammar`.
    pub(crate) fn of(grammar: Grammar) -> Self {
        Self {
            bytes: Vec::new(),
            solidworks: grammar.solidworks_layout(),
            short_image: grammar.short_image_header(),
        }
    }

    /// The bytes written so far.
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Consumes the writer.
    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Raw bytes.
    pub(crate) fn raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.bytes.extend_from_slice(bytes);
        self
    }

    fn pstr(&mut self, text: &str) -> &mut Self {
        self.bytes.push(u8::try_from(text.len()).unwrap());
        self.raw(text.as_bytes())
    }

    fn f32s(&mut self, values: &[f32]) -> &mut Self {
        for value in values {
            self.bytes.extend_from_slice(&value.to_le_bytes());
        }
        self
    }

    /// The header block with `payload` bytes after the length.
    pub(crate) fn header(&mut self, payload: usize) -> &mut Self {
        self.bytes.push(b'[');
        self.bytes
            .extend_from_slice(&u32::try_from(payload).unwrap().to_le_bytes());
        let slack = if self.solidworks { 2 } else { 1 };
        self.bytes
            .extend(std::iter::repeat_n(0xbe, payload + slack));
        self.bytes.push(b']');
        self
    }

    /// The camera record.
    pub(crate) fn camera(&mut self, name: &str) -> &mut Self {
        self.bytes.extend_from_slice(&[b'}', 0]);
        self.f32s(&[0.0; 11]);
        self.pstr(name)
    }

    /// A user option record.
    pub(crate) fn user_option(&mut self, text: &str) -> &mut Self {
        self.bytes.push(b'U');
        self.bytes
            .extend_from_slice(&u16::try_from(text.len()).unwrap().to_le_bytes());
        self.raw(text.as_bytes())
    }

    /// The line style table with one entry. A `SolidWorks` stream writes the
    /// entry without the table opcode.
    pub(crate) fn line_styles(&mut self) -> &mut Self {
        if self.solidworks {
            self.bytes.push(0x4a);
        } else {
            self.bytes.extend_from_slice(&[b'h', 1, 0x4a]);
        }
        self.pstr("edw-point");
        self.bytes.extend_from_slice(&3_u16.to_le_bytes());
        self.raw(b"0 1")
    }

    /// Opens a segment.
    pub(crate) fn open(&mut self, name: &str) -> &mut Self {
        self.bytes.push(b'(');
        self.pstr(name)
    }

    /// Closes a segment or side scope.
    pub(crate) fn close(&mut self) -> &mut Self {
        self.raw(b")")
    }

    /// An include record.
    pub(crate) fn include(&mut self, path: &str) -> &mut Self {
        self.bytes.push(b'<');
        self.pstr(path)
    }

    /// A plain style record.
    pub(crate) fn style(&mut self, path: &str) -> &mut Self {
        self.bytes.push(b'{');
        self.pstr(path)
    }

    /// A conditional style record.
    pub(crate) fn conditional_style(&mut self, path: &str, condition: &str) -> &mut Self {
        self.bytes.extend_from_slice(&[b'{', 0]);
        self.pstr(path);
        self.pstr(condition)
    }

    /// A condition record.
    pub(crate) fn condition(&mut self, name: &str) -> &mut Self {
        self.bytes.push(b'?');
        self.pstr(name)
    }

    /// A modelling matrix.
    pub(crate) fn modelling_matrix(&mut self, values: &[f32; 12]) -> &mut Self {
        self.bytes.push(b'%');
        self.f32s(values)
    }

    /// A texture matrix.
    pub(crate) fn texture_matrix(&mut self) -> &mut Self {
        self.bytes.push(b'$');
        self.f32s(&[0.5, 0.0, 0.0, 0.0, -0.5, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0])
    }

    /// A colour record.
    pub(crate) fn colour(&mut self, rgb: [u8; 3]) -> &mut Self {
        self.raw(&[b'~', 1, rgb[0], rgb[1], rgb[2]])
    }

    /// A material record.
    pub(crate) fn material(&mut self, text: &str) -> &mut Self {
        self.raw(&[b'"', 1, 1]);
        self.pstr(text)
    }

    /// A `SolidWorks` material record of flags `01 09`: an empty description and
    /// seven extra bytes.
    pub(crate) fn solidworks_material_extra(&mut self, extra: [u8; 7]) -> &mut Self {
        self.raw(&[b'"', 1, 9, 0]);
        self.raw(&extra)
    }

    /// A `SolidWorks` material record with a parenthesised diffuse colour.
    pub(crate) fn solidworks_material(&mut self, rgb: [f32; 3]) -> &mut Self {
        let [r, g, b] = rgb;
        self.material(&format!("diffuse color=(r={r:.6} g={g:.6} b={b:.6})"))
    }

    /// A tag-0 record.
    pub(crate) fn tag0(&mut self) -> &mut Self {
        self.raw(&[b'0', 1, 0, 0, 0])
    }

    /// The `=` chain: a line weight, an edge weight, and a two byte mask
    /// colour.
    pub(crate) fn eq_record(&mut self) -> &mut Self {
        self.raw(b"=").f32s(&[1.0]);
        self.raw(&[0x0e]).f32s(&[1.0]);
        self.raw(&[b'~', 0x9f, 0x70, 0, 0, 0])
    }

    /// A line pattern record `-` with a pattern name.
    pub(crate) fn line_pattern(&mut self, name: &str) -> &mut Self {
        self.raw(&[b'-', 0xff]);
        self.pstr(name)
    }

    /// An edge pattern record `0d` with a pattern name.
    pub(crate) fn edge_pattern(&mut self, name: &str) -> &mut Self {
        self.raw(&[0x0d, 0xff]);
        self.pstr(name)
    }

    /// A hole marker record `@` with a symbol code.
    pub(crate) fn marker(&mut self, code: u8) -> &mut Self {
        self.raw(&[b'@', code])
    }

    /// A wide user option record: a `u16` character count and UTF-16LE text.
    pub(crate) fn wide_user_option(&mut self, text: &str) -> &mut Self {
        let units: Vec<u16> = text.encode_utf16().collect();
        self.bytes.push(0x16);
        self.bytes
            .extend_from_slice(&u16::try_from(units.len()).unwrap().to_le_bytes());
        for unit in units {
            self.bytes.extend_from_slice(&unit.to_le_bytes());
        }
        self
    }

    /// A geometry side object that holds `items`, the item bytes; opens a
    /// scope that [`Self::close`] closes.
    pub(crate) fn side_geometry(&mut self, id: u32, items: &[u8]) -> &mut Self {
        self.bytes.push(b's');
        self.bytes.extend_from_slice(&id.to_le_bytes());
        self.raw(items)
    }

    /// An `H` attribute with masks `[0x8004, 0x0200]`; with mask `[0x0004]`
    /// in a `SolidWorks` stream.
    pub(crate) fn h_attribute(&mut self) -> &mut Self {
        if self.solidworks {
            return self.raw(&[b'H', 0x04, 0x00, 0x04, 0x00, 0x01]);
        }
        self.raw(&[b'H', 0x04, 0x80, 0x00, 0x02, 0x04, 0x00, 0x00, 0x02, 0x01])
    }

    /// An `H` attribute with mask `[0x0002]` and value zero.
    pub(crate) fn solidworks_h_name(&mut self) -> &mut Self {
        self.raw(&[b'H', 0x02, 0x00, 0x00, 0x00])
    }

    /// An `R` attribute with mask `[0x00001000]`; with mask `[0x00100000]`
    /// and two default pairs in a `SolidWorks` stream.
    pub(crate) fn r_attribute(&mut self) -> &mut Self {
        if self.solidworks {
            return self.solidworks_r_run(2);
        }
        self.raw(&[b'R', 0x00, 0x10, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00])
    }

    /// An `R` attribute with mask `[0x00100000]`, a 16 byte head, and `pairs`
    /// pairs `ff 01`.
    pub(crate) fn solidworks_r_run(&mut self, pairs: usize) -> &mut Self {
        self.raw(&[b'R', 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x10, 0x00]);
        self.raw(&[
            4, 0, 0, 0, 4, 0, 0, 0, 0x7f, 0x7f, 0x7f, 3, 0x7f, 0x7f, 0x7f, 3,
        ]);
        for _ in 0..pairs {
            self.raw(&[0xff, 0x01]);
        }
        self
    }

    /// A text-slot `V` record.
    pub(crate) fn v_text(&mut self) -> &mut Self {
        let tail = if self.solidworks { 0x03 } else { 0x07 };
        self.raw(&[b'V', 0xff, 0x00, 0xff, 0xff, 0x00, 0x00, tail, 0x00])
    }

    /// A texture definition. A `SolidWorks` stream writes the record `t` and
    /// then a `V` of two bytes.
    pub(crate) fn v_texture(&mut self, name: &str) -> &mut Self {
        if self.solidworks {
            self.raw(b"t");
            self.pstr(name);
            self.pstr(name);
            self.raw(&[0x01, 0x80, 0x02, 0x00, 0x01, 0x01]);
            return self.raw(&[b'V', 0x40, 0x00]);
        }
        self.raw(&[b'V', 0x40, 0x00, 0x74]);
        self.pstr(name);
        self.pstr(name);
        self.raw(&[0x01, 0xc0, 0x02, 0x00, 0x01, 0x01])
    }

    /// An object slot.
    pub(crate) fn slot(&mut self) -> &mut Self {
        self.raw(b"q")
    }

    /// A shell placeholder.
    pub(crate) fn shell_placeholder(&mut self) -> &mut Self {
        self.raw(&[b'S', 0x90, 0x02, 0x00, 0x00])
    }

    /// An inline shell with object id `id`.
    pub(crate) fn shell(&mut self, id: u32, points: &[[f32; 3]], strips: &[Vec<u32>]) -> &mut Self {
        self.shell_with_tail(id, points, strips, &[])
    }

    /// An inline shell whose bytes between the normals and the terminator
    /// are `tail`.
    pub(crate) fn shell_with_tail(
        &mut self,
        id: u32,
        points: &[[f32; 3]],
        strips: &[Vec<u32>],
        tail: &[u8],
    ) -> &mut Self {
        self.raw(&[b'S', 0x0e]);
        self.bytes.extend_from_slice(&id.to_le_bytes());
        self.bytes.push(0);
        self.bytes
            .extend_from_slice(&u32::try_from(points.len()).unwrap().to_le_bytes());
        for point in points {
            self.f32s(point);
        }
        let wide = points.len() >= WIDE_INDEX_FROM;
        let mut list = vec![if wide { 16_u8 } else { 8 }];
        for strip in strips {
            let mut values = vec![u32::try_from(strip.len()).unwrap()];
            values.extend_from_slice(strip);
            for value in values {
                if wide {
                    list.extend_from_slice(&u16::try_from(value).unwrap().to_le_bytes());
                } else {
                    list.push(u8::try_from(value).unwrap());
                }
            }
        }
        self.bytes.push(1);
        self.bytes
            .extend_from_slice(&u32::try_from(list.len()).unwrap().to_le_bytes());
        self.raw(&list);
        let normals = 12 * (points.len() / 4) + 4 * (points.len() % 4);
        self.raw(&[1, 7, 12]);
        self.bytes
            .extend_from_slice(&u32::try_from(normals).unwrap().to_le_bytes());
        self.bytes.extend(std::iter::repeat_n(0x40, normals));
        self.raw(tail);
        self.bytes.push(0);
        self
    }

    /// A shell instance of object `source` with object id `own`.
    pub(crate) fn instance(&mut self, source: u64, own: u64, values: &[f32; 12]) -> &mut Self {
        self.bytes.push(b'&');
        self.bytes.extend_from_slice(&source.to_le_bytes());
        self.bytes.extend_from_slice(&own.to_le_bytes());
        self.bytes.push(1);
        self.f32s(values)
    }

    /// A text side object; opens a scope that [`Self::close`] closes.
    pub(crate) fn side_text(&mut self, id: u32, text: &str) -> &mut Self {
        self.bytes.push(b's');
        self.bytes.extend_from_slice(&id.to_le_bytes());
        self.bytes.push(b'x');
        self.bytes.extend_from_slice(&[0; 12]);
        self.bytes.push(5);
        let units: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        if units.len() >= 0xfe {
            self.bytes.push(0xfe);
            self.bytes
                .extend_from_slice(&u16::try_from(units.len()).unwrap().to_le_bytes());
        } else {
            self.bytes.push(u8::try_from(units.len()).unwrap());
        }
        self.raw(&units);
        self.bytes.push(0);
        self
    }

    /// An empty side object.
    pub(crate) fn side_empty(&mut self, id: u32) -> &mut Self {
        self.bytes.push(b's');
        self.bytes.extend_from_slice(&id.to_le_bytes());
        self
    }

    /// An image side object holding `image`.
    pub(crate) fn side_image(&mut self, id: u32, image: &[u8]) -> &mut Self {
        self.bytes.push(b's');
        self.bytes.extend_from_slice(&id.to_le_bytes());
        self.bytes.push(b'i');
        self.bytes.extend_from_slice(&[0; 12]);
        if self.short_image {
            self.bytes.push(0xa3);
        } else {
            self.bytes.extend_from_slice(&163_u32.to_le_bytes());
        }
        self.pstr("tex");
        self.raw(&[0, 2, 0, 0, 0, 2, 0, 0, 2]);
        self.bytes
            .extend_from_slice(&u32::try_from(image.len()).unwrap().to_le_bytes());
        self.raw(image);
        self.raw(&[0x00, 0xff])
    }

    /// A separator byte.
    pub(crate) fn separator(&mut self) -> &mut Self {
        self.raw(&[1])
    }

    /// The trailer and the end byte.
    pub(crate) fn trailer_and_end(&mut self) -> &mut Self {
        let code = if self.solidworks { 0x2c } else { 0x33 };
        self.raw(&[b'[', 9, 0, 0, 0, 0xbe, code]);
        self.raw(&[0; 15]);
        self.raw(b"]z")
    }
}

/// One item of a body sub-segment.
#[derive(Debug, Clone)]
pub(crate) enum Item {
    /// An inline shell: points in metres and strips.
    Shell {
        /// Vertices.
        points: Vec<[f32; 3]>,
        /// Triangle strips.
        strips: Vec<Vec<u32>>,
    },
    /// An instance of an earlier item of the same body.
    Instance {
        /// Index of the source item within the body (over all sub-segments).
        source: usize,
        /// The affine.
        matrix: [f32; 12],
    },
}

/// A library body.
#[derive(Debug, Clone, Default)]
pub(crate) struct BodySpec {
    /// Body colour.
    pub(crate) colour: Option<[u8; 3]>,
    /// Sub-segments with their items.
    pub(crate) subs: Vec<Vec<Item>>,
}

/// A tree node.
#[derive(Debug, Clone)]
pub(crate) enum NodeSpec {
    /// A placed instance of a library body.
    Part {
        /// Segment name, such as `assempart0`.
        segment: String,
        /// Display name.
        name: String,
        /// Library body index.
        body: usize,
        /// The modelling matrix, when the part has one.
        matrix: Option<[f32; 12]>,
        /// Whether the node carries a `name=` user option.
        user_name: bool,
    },
    /// A sub-assembly.
    Sub {
        /// Segment name, such as `subassem3`.
        segment: String,
        /// Display name.
        name: String,
        /// Children.
        children: Vec<NodeSpec>,
    },
}

/// A whole model.
#[derive(Debug, Clone, Default)]
pub(crate) struct ModelSpec {
    /// Document name.
    pub(crate) document: String,
    /// Library bodies.
    pub(crate) bodies: Vec<BodySpec>,
    /// Top-level nodes.
    pub(crate) tree: Vec<NodeSpec>,
    /// The active display state and its `(path, visible)` components.
    pub(crate) state: Option<(String, Vec<(String, bool)>)>,
}

/// The library path of body `index`.
pub(crate) fn body_path(index: usize) -> String {
    format!("/include library/model_1_include/shared/body{index}")
}

struct Writer<'a> {
    spec: &'a ModelSpec,
    out: Stream,
    next_id: u32,
    store: Vec<(u32, Vec<u8>)>,
    defined: Vec<bool>,
}

impl Writer<'_> {
    fn text_slot(&mut self, segment: &str, text: &str) {
        self.out.open(segment).v_text().close().slot();
        let id = self.next_id;
        self.next_id += 1;
        let mut object = Stream::new();
        object.side_text(id, text);
        self.store.push((id, object.into_bytes()));
    }

    fn reference(&mut self, name: &str) {
        self.out.open("reference");
        self.text_slot("refname", name);
        self.text_slot("path", "C:\\private\\source.sldprt");
        self.out.close();
    }

    fn body(&mut self, index: usize) {
        let Some(body) = self.spec.bodies.get(index) else {
            return;
        };
        let path = body_path(index);
        if !self.defined.get(index).copied().unwrap_or(true) {
            if let Some(flag) = self.defined.get_mut(index) {
                *flag = true;
            }
            self.out.open(&path);
            if let Some(rgb) = body.colour {
                self.out.colour(rgb);
            }
            let mut ids: Vec<u32> = Vec::new();
            for (sub, items) in body.subs.iter().enumerate() {
                self.out.open(&format!("body{index}sub{sub}"));
                for item in items {
                    let id = self.next_id;
                    self.next_id += 1;
                    self.out.shell_placeholder().slot();
                    let mut object = Stream::new();
                    match item {
                        Item::Shell { points, strips } => {
                            object.shell(id, points, strips);
                        }
                        Item::Instance { source, matrix } => {
                            let source_id = ids.get(*source).copied().unwrap_or(0);
                            object.instance(u64::from(source_id), u64::from(id), matrix);
                        }
                    }
                    ids.push(id);
                    self.store.push((id, object.into_bytes()));
                }
                self.out.close();
            }
            self.out.close();
        }
        self.out.include(&path);
    }

    fn node(&mut self, node: &NodeSpec) {
        match node {
            NodeSpec::Part {
                segment,
                name,
                body,
                matrix,
                user_name,
            } => {
                self.out.open(segment);
                if *user_name {
                    self.out.user_option(&format!("name=`{name}`"));
                }
                self.reference(name);
                self.out.open("part");
                if let Some(values) = matrix {
                    self.out.modelling_matrix(values);
                }
                self.body(*body);
                self.out.close().close();
            }
            NodeSpec::Sub {
                segment,
                name,
                children,
            } => {
                self.out.open(segment);
                self.out.user_option(&format!("name=`{name}`"));
                self.reference(name);
                self.text_slot("name", name);
                for child in children {
                    self.node(child);
                }
                self.out.close();
            }
        }
    }
}

impl ModelSpec {
    /// The inflated model stream.
    pub(crate) fn stream(&self) -> Vec<u8> {
        let mut writer = Writer {
            spec: self,
            out: Stream::new(),
            next_id: 0,
            store: Vec::new(),
            defined: vec![false; self.bodies.len()],
        };
        writer.out.header(2).camera("default");
        writer.out.user_option("length=`3`");
        writer.reference(&self.document);
        writer
            .out
            .open("config0")
            .user_option("config name=`empty`");
        writer.out.condition("disp0");
        writer.out.open("sheet0").open("view0").open("3dmodel");
        writer.out.h_attribute().r_attribute();
        writer.out.open("includes");
        for node in &self.tree {
            writer.node(node);
        }
        writer.out.close().close().close().close();
        if let Some((current, components)) = &self.state {
            writer
                .out
                .open("displaystates")
                .user_option(&format!("default=disp0,current=`{current}`"));
            writer.out.open(current);
            for (index, (path, visible)) in components.iter().enumerate() {
                let vis = if *visible { "on" } else { "off" };
                writer
                    .out
                    .open(&format!("comp{index}"))
                    .user_option(&format!("path={path},vis={vis}"))
                    .close();
            }
            writer.out.close().close();
        }
        writer.out.close();
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

/// Strip vertex order of a quad given as a counter-clockwise loop.
pub(crate) fn quad_strip() -> Vec<u32> {
    vec![0, 1, 3, 2]
}

/// The six faces of an axis-aligned box from the origin to `size` (metres),
/// each a four-vertex shell whose triangles face outward.
pub(crate) fn box_shells(size: [f32; 3]) -> Vec<Item> {
    let [x, y, z] = size;
    let faces: [[[f32; 3]; 4]; 6] = [
        [[0.0, 0.0, 0.0], [0.0, y, 0.0], [x, y, 0.0], [x, 0.0, 0.0]],
        [[0.0, 0.0, z], [x, 0.0, z], [x, y, z], [0.0, y, z]],
        [[0.0, 0.0, 0.0], [x, 0.0, 0.0], [x, 0.0, z], [0.0, 0.0, z]],
        [[0.0, y, 0.0], [0.0, y, z], [x, y, z], [x, y, 0.0]],
        [[0.0, 0.0, 0.0], [0.0, 0.0, z], [0.0, y, z], [0.0, y, 0.0]],
        [[x, 0.0, 0.0], [x, y, 0.0], [x, y, z], [x, 0.0, z]],
    ];
    faces
        .iter()
        .map(|points| Item::Shell {
            points: points.to_vec(),
            strips: vec![quad_strip()],
        })
        .collect()
}

/// The range block of a shell with `vertices` vertices: `03 01 02`, two
/// `(u, v)` pairs, `08`, and one 16-bit value per vertex.
pub(crate) fn range_block(vertices: usize, v: [f32; 2]) -> Vec<u8> {
    let mut block = vec![3, 1, 2];
    for value in [0.0_f32, v[0], 0.0, v[1]] {
        block.extend_from_slice(&value.to_le_bytes());
    }
    block.push(8);
    block.extend_from_slice(&u32::try_from(2 * vertices).unwrap().to_le_bytes());
    block.extend(std::iter::repeat_n(0xff, 2 * vertices));
    block
}

/// The face tag of a shell: `63`, a narrow or wide user option, and a zero
/// byte.
pub(crate) fn face_tag(text: &str, wide: bool) -> Vec<u8> {
    let mut tag = Stream::new();
    tag.raw(&[0x63]);
    if wide {
        tag.wide_user_option(text);
    } else {
        tag.user_option(text);
    }
    tag.raw(&[0]);
    tag.into_bytes()
}
