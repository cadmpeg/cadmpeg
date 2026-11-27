// SPDX-License-Identifier: Apache-2.0
//! Synthetic flange documents (`docs/formats/solidedge.md` §10.3).

use super::{
    cluster_and_map, document, navigator_body, part_body, put_reals, put_words, slot_object,
    type_table, Object, Slot, EXTENT_CLASSES,
};

/// The class names of a flange document, by form.
const FLANGE_CLASSES: [&str; 19] = [
    EXTENT_CLASSES[0],
    EXTENT_CLASSES[1],
    EXTENT_CLASSES[2],
    EXTENT_CLASSES[3],
    EXTENT_CLASSES[4],
    EXTENT_CLASSES[5],
    EXTENT_CLASSES[6],
    EXTENT_CLASSES[7],
    EXTENT_CLASSES[8],
    EXTENT_CLASSES[9],
    EXTENT_CLASSES[10],
    EXTENT_CLASSES[11],
    EXTENT_CLASSES[12],
    EXTENT_CLASSES[13],
    EXTENT_CLASSES[14],
    EXTENT_CLASSES[15],
    EXTENT_CLASSES[16],
    "CLSID_JMTopologyProxy",
    "Assoc subsystem Standard Relation implementation",
];
const PROXY_FORM: u16 = 17;
const RELATION_FORM: u16 = 18;
/// Bytes of a relation object.
const RELATION_LEN: usize = 161;
/// The first feature object and the first slot object of a flange document.
const FIRST_FEATURE: u32 = 10;
const SLOT_BASE: u32 = 1000;
/// Identifiers spent on the slot objects of one flange.
const SLOTS_PER_FLANGE: u32 = 20_000;
/// The distance from a flange object's identifier to its relation's.
const RELATION_OFFSET: u32 = 10_000;

/// One flange of [`flange_document`], in metres and radians.
#[derive(Clone)]
pub(crate) struct FlangeSpec {
    /// The extent kind: 134 full edge, 135 centred partial width, 138 offset
    /// and width.
    pub(crate) extent: f64,
    /// The partial width, stored with extent kinds 135 and 138.
    pub(crate) partial_width: f64,
    /// The offset, stored with extent kind 138.
    pub(crate) offset: f64,
    pub(crate) length: f64,
    /// The entered angle.
    pub(crate) angle: f64,
    pub(crate) side: f64,
    /// The material side: 1 inside, 2 outside, 9 bend outside.
    pub(crate) material_side: f64,
    pub(crate) radius: f64,
    pub(crate) relief_width: f64,
    pub(crate) relief_length: f64,
    pub(crate) relief_type: f64,
    /// Whether a relation binds the radius to the document variable.
    pub(crate) radius_bound: bool,
    /// The size of the edge proxy; 559 and above.
    pub(crate) proxy_len: usize,
    pub(crate) edge_start: [f64; 3],
    pub(crate) edge_direction: [f64; 3],
    pub(crate) edge_length: f64,
}

impl Default for FlangeSpec {
    fn default() -> Self {
        Self {
            extent: 134.0,
            partial_width: 0.02,
            offset: 0.005,
            length: 0.01,
            angle: std::f64::consts::FRAC_PI_2,
            side: 2.0,
            material_side: 1.0,
            radius: 0.003,
            relief_width: 0.001,
            relief_length: 0.0,
            relief_type: 53.0,
            radius_bound: true,
            proxy_len: 559,
            edge_start: [0.04, 0.03, 0.001],
            edge_direction: [-1.0, 0.0, 0.0],
            edge_length: 0.04,
        }
    }
}

fn proxy_body(spec: &FlangeSpec) -> Vec<u8> {
    let mut body = vec![0_u8; spec.proxy_len - 8];
    let base = spec.proxy_len - 438;
    put_reals(&mut body, base, &spec.edge_start);
    put_reals(&mut body, base + 24, &spec.edge_direction);
    put_reals(&mut body, base + 56, &[spec.edge_length]);
    body
}

/// Store the identifier of the object `slot` states `after` bytes past the
/// function name at `end`, and return that identifier.
fn put_slot(
    body: &mut [u8],
    end: usize,
    base: u32,
    after: usize,
    slot: Slot,
    objects: &mut Vec<Object>,
) -> u32 {
    let object_id = base + u32::try_from(after).unwrap();
    if let Some(object) = slot_object(object_id, slot) {
        body[end + after..end + after + 4].copy_from_slice(&object_id.to_le_bytes());
        objects.push(object);
    }
    object_id
}

fn relation_body(own: u32, variable: u32) -> Vec<u8> {
    let mut body = vec![0_u8; RELATION_LEN - 8];
    put_words(&mut body, 99, &[own]);
    put_words(&mut body, 127, &[variable]);
    body
}

/// A document whose cluster 0 holds one `JMMultiFlangeFunction` feature per
/// spec, in order, from object 10. Each flange names the flange before it at
/// slot 40. The slots follow §10.3 and shift by 24 bytes from offset 492 for
/// extent kind 135 and by 44 bytes for extent kind 138. The streams `streams` stand beside the cluster.
pub(crate) fn flange_document(specs: &[FlangeSpec], streams: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut objects = Vec::new();
    for id in 1..=3 {
        objects.push(Object {
            id,
            form: 0,
            flags: 0,
            body: vec![0; 8],
        });
    }
    objects.push(Object {
        id: 7,
        form: 3,
        flags: 0,
        body: vec![0; 8],
    });
    let mut listed = Vec::new();
    for (index, spec) in specs.iter().enumerate() {
        let index = u32::try_from(index).unwrap();
        let id = FIRST_FEATURE + index;
        listed.push(id);
        let base = SLOT_BASE + index * SLOTS_PER_FLANGE;
        let mut body = part_body("JMMultiFlangeFunction");
        let end = body.len();
        body.resize(end + 900 + 4, 0);
        let shift = match spec.extent {
            135.0 => 24,
            138.0 => 44,
            _ => 0,
        };
        let previous = if index == 0 { 0 } else { id - 1 };
        body[end + 40..end + 44].copy_from_slice(&previous.to_le_bytes());
        let proxy = base + 72;
        body[end + 72..end + 76].copy_from_slice(&proxy.to_le_bytes());
        objects.push(Object {
            id: proxy,
            form: PROXY_FORM,
            flags: 0,
            body: proxy_body(spec),
        });
        put_slot(
            &mut body,
            end,
            base,
            100,
            Slot::Dim(spec.length),
            &mut objects,
        );
        put_slot(
            &mut body,
            end,
            base,
            128,
            Slot::Dim(spec.angle),
            &mut objects,
        );
        put_slot(&mut body, end, base, 156, Slot::Double(0.001), &mut objects);
        put_slot(
            &mut body,
            end,
            base,
            184,
            Slot::Double(spec.extent),
            &mut objects,
        );
        put_slot(
            &mut body,
            end,
            base,
            212,
            Slot::Double(spec.material_side),
            &mut objects,
        );
        put_slot(
            &mut body,
            end,
            base,
            240,
            Slot::Double(spec.side),
            &mut objects,
        );
        let radius = put_slot(
            &mut body,
            end,
            base,
            296,
            Slot::Double(spec.radius),
            &mut objects,
        );
        let width = put_slot(
            &mut body,
            end,
            base,
            380,
            Slot::Double(spec.relief_width),
            &mut objects,
        );
        let length = put_slot(
            &mut body,
            end,
            base,
            408,
            Slot::Double(spec.relief_length),
            &mut objects,
        );
        put_slot(&mut body, end, base, 440, Slot::Double(44.0), &mut objects);
        if shift == 24 {
            put_slot(
                &mut body,
                end,
                base,
                492,
                Slot::Dim(spec.partial_width),
                &mut objects,
            );
        }
        if shift == 44 {
            put_slot(
                &mut body,
                end,
                base,
                492,
                Slot::Dim(spec.offset),
                &mut objects,
            );
            put_slot(
                &mut body,
                end,
                base,
                512,
                Slot::Dim(spec.partial_width),
                &mut objects,
            );
        }
        put_slot(
            &mut body,
            end,
            base,
            496 + shift,
            Slot::Double(spec.relief_type),
            &mut objects,
        );
        let mut bound = vec![(width, 6), (length, 7)];
        if spec.radius_bound {
            bound.push((radius, 5));
        }
        for (own, variable) in bound {
            objects.push(Object {
                id: own + RELATION_OFFSET,
                form: RELATION_FORM,
                flags: 0,
                body: relation_body(own, 2000 + variable),
            });
        }
        objects.push(Object {
            id,
            form: 1,
            flags: 0,
            body,
        });
    }
    objects.push(Object {
        id: 5000,
        form: 4,
        flags: 0,
        body: navigator_body([1, 2, 3], &listed),
    });
    objects.sort_by_key(|object| object.id);
    let (cluster, map) = cluster_and_map(&objects);
    let mut all = vec![
        ("PSMcluster0", cluster),
        ("PSMspacemap/0x00000000", map),
        ("PSMtypetable", type_table(&FLANGE_CLASSES, &[])),
    ];
    all.extend(streams.iter().cloned());
    document(&all)
}
