// SPDX-License-Identifier: Apache-2.0
//! Code-built exact-geometry stream payloads (specification §8).

use crate::layout::token;

/// Index value that names no record.
pub(crate) const NONE: u32 = u32::MAX;

/// Scalar-aware payload bytes of one record.
#[derive(Debug, Clone)]
pub(crate) struct Payload {
    double: bool,
    bytes: Vec<u8>,
}

impl Payload {
    /// An empty payload with `f64` scalars when `double`.
    pub(crate) fn new(double: bool) -> Self {
        Self {
            double,
            bytes: Vec::new(),
        }
    }

    pub(crate) fn u8(mut self, value: u8) -> Self {
        self.bytes.push(value);
        self
    }

    pub(crate) fn u32(mut self, value: u32) -> Self {
        self.bytes.extend_from_slice(&value.to_le_bytes());
        self
    }

    pub(crate) fn i32(mut self, value: i32) -> Self {
        self.bytes.extend_from_slice(&value.to_le_bytes());
        self
    }

    /// One scalar of the stream width.
    pub(crate) fn scalar(mut self, value: f64) -> Self {
        if self.double {
            self.bytes.extend_from_slice(&value.to_le_bytes());
        } else {
            let narrow = cadmpeg_core::convert::f32_from_f64(value).expect("f32 scalar");
            self.bytes.extend_from_slice(&narrow.to_le_bytes());
        }
        self
    }

    pub(crate) fn scalars(self, values: &[f64]) -> Self {
        values
            .iter()
            .fold(self, |payload, value| payload.scalar(*value))
    }

    pub(crate) fn bytes(self) -> Vec<u8> {
        self.bytes
    }
}

/// An exact stream under construction.
#[derive(Debug, Clone)]
pub(crate) struct Stream {
    double: bool,
    bytes: Vec<u8>,
}

impl Stream {
    /// A stream with its stream index record: version 4 for `f64`
    /// scalars, version 3 for `f32`.
    pub(crate) fn new(double: bool) -> Self {
        let mut stream = Self {
            double,
            bytes: Vec::new(),
        };
        let version = if double { 4 } else { 3 };
        stream.record(token::STREAM_EDGE_TESSELLATION, &version_payload(version));
        stream
    }

    /// The stream identity matching the scalar width.
    pub(crate) fn id(&self) -> u32 {
        if self.double {
            token::STREAM_EXACT_F64
        } else {
            token::STREAM_EXACT_F32
        }
    }

    pub(crate) fn payload(&self) -> Payload {
        Payload::new(self.double)
    }

    pub(crate) fn record(&mut self, tag: impl Into<u32>, payload: &[u8]) -> &mut Self {
        self.bytes.extend_from_slice(&tag.into().to_le_bytes());
        let length = u32::try_from(payload.len()).expect("record length");
        self.bytes.extend_from_slice(&length.to_le_bytes());
        self.bytes.extend_from_slice(payload);
        self
    }

    pub(crate) fn terminator(&mut self) -> &mut Self {
        self.record(0_u32, &[])
    }

    pub(crate) fn loop_record(&mut self, marker: i32) -> &mut Self {
        let payload = self.payload().i32(marker).bytes();
        self.record(token::EXACT_LOOP, &payload)
    }

    pub(crate) fn coedge(
        &mut self,
        edge: u32,
        orientation: u8,
        pcurve: u32,
        secondary: u32,
    ) -> &mut Self {
        let payload = self
            .payload()
            .u32(edge)
            .u8(orientation)
            .u32(pcurve)
            .u32(secondary)
            .bytes();
        self.record(token::EXACT_COEDGE, &payload)
    }

    /// A plane record `O + u·U + v·V` with normal `N` and no range pairs.
    pub(crate) fn plane(
        &mut self,
        origin: [f64; 3],
        u: [f64; 3],
        v: [f64; 3],
        normal: [f64; 3],
    ) -> &mut Self {
        let payload = self
            .payload()
            .u8(5)
            .u8(0)
            .u8(0)
            .scalars(&origin)
            .scalars(&u)
            .scalars(&v)
            .scalars(&normal)
            .u8(0)
            .bytes();
        self.record(token::EXACT_SURFACE, &payload)
    }

    /// A line edge curve `O + t·D`.
    pub(crate) fn line_edge(&mut self, origin: [f64; 3], direction: [f64; 3]) -> &mut Self {
        let payload = self
            .payload()
            .u8(5)
            .u8(0)
            .scalars(&origin)
            .scalars(&direction)
            .u8(0)
            .bytes();
        self.record(token::EXACT_EDGE_CURVE, &payload)
    }

    /// A line pcurve `O + t·D`.
    pub(crate) fn line_pcurve(&mut self, origin: [f64; 2], direction: [f64; 2]) -> &mut Self {
        let payload = self
            .payload()
            .u8(5)
            .u8(0)
            .scalars(&origin)
            .scalars(&direction)
            .u8(0)
            .bytes();
        self.record(token::EXACT_PCURVE, &payload)
    }

    /// The stream payload with its closing stream terminator.
    pub(crate) fn finish(mut self) -> Vec<u8> {
        self.terminator();
        self.bytes
    }

    /// The stream payload without a closing stream terminator.
    pub(crate) fn unterminated(self) -> Vec<u8> {
        self.bytes
    }
}

fn version_payload(version: u32) -> Vec<u8> {
    version.to_le_bytes().to_vec()
}

/// Corners of the cube `[0, s]³`, `s` = 0.01 m.
pub(crate) const CUBE_SIZE: f64 = 0.01;

/// Cube corner `index` with bit 0 = x, bit 1 = y, bit 2 = z.
pub(crate) fn cube_corner(index: usize) -> [f64; 3] {
    let bit = |shift: usize| {
        if index >> shift & 1 == 1 {
            CUBE_SIZE
        } else {
            0.0
        }
    };
    [bit(0), bit(1), bit(2)]
}

/// Faces of the cube as corner rings counter-clockwise seen from outside.
pub(crate) const CUBE_FACES: [[usize; 4]; 6] = [
    [0, 2, 3, 1],
    [4, 5, 7, 6],
    [0, 1, 5, 4],
    [2, 6, 7, 3],
    [0, 4, 6, 2],
    [1, 3, 7, 5],
];

/// A closed cube stream. Face `f` has its plane `U` and `V` along the ring
/// and stores its loop counter-clockwise in its own `(u, v)` domain, as the
/// format does; `inward[f]` swaps `U` and `V`, so that `U × V` and the
/// stored loop direction point into the cube. `reversed_pcurve[f]` stores
/// every pcurve of face `f` against the loop direction.
pub(crate) fn cube_stream(double: bool, inward: [bool; 6], reversed_pcurve: [bool; 6]) -> Stream {
    cube_with_sheet(double, inward, reversed_pcurve, false)
}

/// The closed cube of [`cube_stream`] followed by one open square face of
/// side [`CUBE_SIZE`] in the plane `z = 2s`, which bounds no volume.
pub(crate) fn cube_and_sheet_stream(double: bool) -> Stream {
    cube_with_sheet(double, [false; 6], [false; 6], true)
}

fn cube_with_sheet(
    double: bool,
    inward: [bool; 6],
    reversed_pcurve: [bool; 6],
    sheet: bool,
) -> Stream {
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for ring in CUBE_FACES {
        for position in 0..4 {
            let (a, b) = (ring[position], ring[(position + 1) % 4]);
            let key = (a.min(b), a.max(b));
            if !edges.contains(&key) {
                edges.push(key);
            }
        }
    }
    let mut stream = Stream::new(double);
    let mut pcurves: Vec<([f64; 2], [f64; 2])> = Vec::new();
    for (face, ring) in CUBE_FACES.iter().enumerate() {
        let ring: Vec<usize> = if inward[face] {
            vec![ring[0], ring[3], ring[2], ring[1]]
        } else {
            ring.to_vec()
        };
        let origin = cube_corner(ring[0]);
        let u = sub(cube_corner(ring[1]), origin);
        let v = sub(cube_corner(ring[3]), origin);
        stream.plane(origin, u, v, cross(u, v));
        stream.loop_record(-2);
        let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        for position in 0..4 {
            let (a, b) = (ring[position], ring[(position + 1) % 4]);
            let edge = edges
                .iter()
                .position(|edge| *edge == (a.min(b), a.max(b)))
                .expect("cube edge");
            let orientation = u8::from(a < b);
            let (start, end) = (uv[position], uv[(position + 1) % 4]);
            let (from, to) = if reversed_pcurve[face] {
                (end, start)
            } else {
                (start, end)
            };
            let pcurve = u32::try_from(pcurves.len()).expect("pcurve index");
            pcurves.push((from, [to[0] - from[0], to[1] - from[1]]));
            stream.coedge(
                u32::try_from(edge).expect("edge index"),
                orientation,
                pcurve,
                NONE,
            );
        }
        stream.terminator();
    }
    let height = 2.0 * CUBE_SIZE;
    let square = [
        [0.0, 0.0],
        [CUBE_SIZE, 0.0],
        [CUBE_SIZE, CUBE_SIZE],
        [0.0, CUBE_SIZE],
    ];
    if sheet {
        stream.plane(
            [0.0, 0.0, height],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        );
        stream.loop_record(-2);
        for position in 0..4 {
            let (start, end) = (square[position], square[(position + 1) % 4]);
            let edge = u32::try_from(edges.len() + position).expect("edge index");
            let pcurve = u32::try_from(pcurves.len()).expect("pcurve index");
            pcurves.push((start, [end[0] - start[0], end[1] - start[1]]));
            stream.coedge(edge, 1, pcurve, NONE);
        }
        stream.terminator();
    }
    for (a, b) in &edges {
        let start = cube_corner(*a);
        stream.line_edge(start, sub(cube_corner(*b), start));
    }
    if sheet {
        for position in 0..4 {
            let (start, end) = (square[position], square[(position + 1) % 4]);
            stream.line_edge(
                [start[0], start[1], height],
                [end[0] - start[0], end[1] - start[1], 0.0],
            );
        }
    }
    for (origin, direction) in pcurves {
        stream.line_pcurve(origin, direction);
    }
    stream
}

/// Stores every edge of `model` that two coedges use once per use: the
/// second use names a new record with the same curve, reversed when
/// `reverse`. Returns the new record of each shared edge.
pub(crate) fn split_shared_edges(
    model: &mut crate::brep::model::ExactModel,
    reverse: bool,
) -> std::collections::BTreeMap<usize, usize> {
    let mut seen = std::collections::BTreeSet::new();
    let mut copies = std::collections::BTreeMap::new();
    for loop_index in 0..model.loops.len() {
        for position in 0..model.loops[loop_index].coedges.len() {
            let Some(edge) = model.loops[loop_index].coedges[position].edge else {
                continue;
            };
            if seen.insert(edge) {
                continue;
            }
            let mut copy = model.edges[edge].clone();
            if let (true, crate::brep::model::CurveGeometry::Line { origin, direction }) =
                (reverse, &copy.geometry)
            {
                copy.geometry = crate::brep::model::CurveGeometry::Line {
                    origin: [
                        origin[0] + direction[0],
                        origin[1] + direction[1],
                        origin[2] + direction[2],
                    ],
                    direction: direction.map(|value| -value),
                };
            }
            copies.insert(edge, model.edges.len());
            model.loops[loop_index].coedges[position].edge = Some(model.edges.len());
            model.edges.push(copy);
        }
    }
    copies
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
