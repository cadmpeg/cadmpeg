// SPDX-License-Identifier: Apache-2.0
//! The model cache: the evaluated bodies a design container keeps.
//!
//! `cache/model.json` is the manifest. It keys the cache to the document it
//! was computed for (`document`, the SHA-256 of `document.json`) and to its
//! payload (`payload`, the SHA-256 of `cache/bodies.bin`), and it lists the
//! payload's `slices` as contiguous `[offset, length]` pairs. Its `bodies`
//! table gives each model body's slice. Its `model` records hold the
//! evaluation: each feature's `state`, each state's model bodies as runs of
//! `[first, count]`, and each model body's table entry, key, and name.
//!
//! The final state is the state of the last feature that was not skipped.
//! Its bodies decode into neutral topology; the other states stay in the
//! retained cache. A slice that starts with `SCBREP05` is a native B-rep
//! body; one that starts with `SCMESH01` is a mesh body, written as a body
//! with its triangles as a tessellation.

mod brep;
mod carrier;
mod emit;
mod mesh;
mod value;

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::{AnnotationBuilder, Annotations};
use cadmpeg_ir::document::{CadIr, EntityRewrite, Topology};
use cadmpeg_ir::ids::{BodyId, IdentityKey};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::tessellation::{Tessellation, TessellationId};
use cadmpeg_ir::topology::{Body, BodyKind, Color};
use serde_json::{Map, Value};

use crate::loss::SolidcraftLossCode;
use crate::package::Package;

/// The cache manifest member.
pub(crate) const MANIFEST_MEMBER: &str = "cache/model.json";
/// The cache payload member.
pub(crate) const PAYLOAD_MEMBER: &str = "cache/bodies.bin";
/// The cache format this decoder reads.
const CACHE_FORMAT: &str = "solidcraft-model-cache/10";

/// What decoding the model cache gave.
#[derive(Debug, Default)]
pub(crate) struct Outcome {
    /// Bodies written to the neutral topology.
    pub(crate) bodies: usize,
    /// The losses of the cache.
    pub(crate) losses: Vec<LossNote>,
    /// The bodies each feature made, by feature id.
    pub(crate) outputs: BTreeMap<u64, Vec<BodyId>>,
    /// The bodies each component owns, by component id; 0 is the root.
    pub(crate) components: BTreeMap<u64, Vec<BodyId>>,
}

/// One model body of the final state.
struct ModelBody<'a> {
    /// The model body's persistent key.
    key: IdentityKey,
    /// The model body's name.
    name: Option<String>,
    /// Its slice of the payload.
    slice: View<'a>,
    /// The feature that made it.
    feature: Option<u64>,
    /// The component that owns it; 0 is the root.
    component: u64,
    /// The persistent name of each face, in face order.
    face_names: Vec<String>,
}

fn malformed(what: &str) -> CodecError {
    CodecError::malformed(format_args!("model cache: {what}"))
}

fn count(value: Option<&Value>, what: &str) -> Result<usize, CodecError> {
    value
        .and_then(Value::as_u64)
        .and_then(|number| usize::try_from(number).ok())
        .ok_or_else(|| malformed(what))
}

fn list<'v>(value: &'v Value, key: &str) -> Result<&'v Vec<Value>, CodecError> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| malformed(key))
}

/// Decodes the final state of the model cache into `ir`.
///
/// A cache that another document computed, or one that does not follow the
/// cache format, decodes no body and is named by a loss.
pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    package: &Package<'_>,
    document: View<'_>,
    object: Option<&Map<String, Value>>,
    ir: &mut CadIr,
    annotations: &mut Annotations,
) -> Result<Outcome, CodecError> {
    let appearances = object.and_then(|object| object.get("appearances"));
    let mut looks = crate::look::Looks::new(object);
    let mut outcome = Outcome::default();
    let selected = match select(ctx, package, document) {
        Ok(Selection::Bodies { bodies, states }) => {
            if states > 1 {
                outcome
                    .losses
                    .push(SolidcraftLossCode::ModelCacheUndecoded.note(format!(
                    "{} of the {states} cached model states, those before the final state, are \
                     retained without decoding",
                    states - 1
                )));
            }
            bodies
        }
        Ok(Selection::Stale) => {
            outcome
                .losses
                .push(SolidcraftLossCode::ModelCacheStale.note(
                    "the model cache was computed for another document.json; its bodies are \
                 retained without decoding",
                ));
            return Ok(outcome);
        }
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(error) => {
            outcome
                .losses
                .push(SolidcraftLossCode::ModelCacheMalformed.note(format!(
                    "the model cache does not follow {CACHE_FORMAT} ({error}); it is retained \
                 without decoding"
                )));
            return Ok(outcome);
        }
    };
    let mut tally = emit::Tally::default();
    let mut undecoded = Vec::new();
    for body in selected {
        let mut geometry = Topology::default();
        let mut body_builder = AnnotationBuilder::new();
        let mut body_tally = emit::Tally::default();
        if body.slice.window().starts_with(mesh::SIGNATURE) {
            match mesh_body(ctx, &body, &mut ir.model.tessellations) {
                Ok(record) => {
                    if let Some(feature) = body.feature {
                        outcome
                            .outputs
                            .entry(feature)
                            .or_default()
                            .push(record.id.clone());
                    }
                    outcome
                        .components
                        .entry(body.component)
                        .or_default()
                        .push(record.id.clone());
                    let mut record = record;
                    looks.body(&mut record, &[], &body.face_names);
                    ir.geometry.bodies.push(record);
                    outcome.bodies += 1;
                }
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(error) => undecoded.push(format!(
                    "{} ({error})",
                    body.name.as_deref().unwrap_or("unnamed")
                )),
            }
            continue;
        }
        let emitted = body_record(body.slice).and_then(|saved| {
            emit::emit(
                ctx,
                &saved.solid,
                &body.key,
                body.name.clone(),
                &mut emit::Sink {
                    geometry: &mut geometry,
                    annotations: &mut body_builder,
                    tally: &mut body_tally,
                },
            )
        });
        match emitted {
            Ok(Some(id)) => {
                if let Some(feature) = body.feature {
                    outcome.outputs.entry(feature).or_default().push(id.clone());
                }
                outcome
                    .components
                    .entry(body.component)
                    .or_default()
                    .push(id);
                name_faces(&mut geometry, &body);
                paint(&mut geometry, &body, appearances);
                let faces: Vec<String> = geometry
                    .faces
                    .iter()
                    .map(|face| face.id.to_string())
                    .collect();
                if let Some(record) = geometry.bodies.first_mut() {
                    looks.body(record, &faces, &body.face_names);
                }
                ir.geometry.extend_rewritten(geometry, &mut Unchanged)?;
                annotations.append(body_builder.build())?;
                tally.merge(body_tally);
                outcome.bodies += 1;
            }
            Ok(None) => {}
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => undecoded.push(format!(
                "{} ({error})",
                body.name.as_deref().unwrap_or("unnamed")
            )),
        }
    }
    let mut look_annotations = AnnotationBuilder::new();
    looks.finish(ir, &mut look_annotations)?;
    annotations.append(look_annotations.build())?;
    outcome.losses.extend(tally.notes());
    if !undecoded.is_empty() {
        outcome
            .losses
            .push(SolidcraftLossCode::BodyUndecoded.note(format!(
                "{} cached bodies of the final state did not decode into neutral topology and are \
             retained: {}",
                undecoded.len(),
                undecoded.join(", ")
            )));
    }
    Ok(outcome)
}

/// Decodes one body slice into a scratch topology: the fuzz probe of the
/// slice parsers and the emitter.
pub(crate) fn probe_body(ctx: &DecodeContext<'_>, slice: View<'_>) -> Result<(), CodecError> {
    if slice.window().starts_with(mesh::SIGNATURE) {
        return mesh::read(ctx, slice).map(|_| ());
    }
    let saved = body_record(slice)?;
    let mut geometry = Topology::default();
    let mut builder = AnnotationBuilder::new();
    let mut tally = emit::Tally::default();
    emit::emit(
        ctx,
        &saved.solid,
        &IdentityKey::from(0_usize),
        None,
        &mut emit::Sink {
            geometry: &mut geometry,
            annotations: &mut builder,
            tally: &mut tally,
        },
    )
    .map(|_| ())
}

/// The colour of an appearance look: `color` as eight-bit red, green, and
/// blue, and `opacity` in `[0, 1]`.
fn look_color(look: &Value) -> Option<Color> {
    let channels = look.get("color")?.as_array()?;
    let [r, g, b] = channels.as_slice() else {
        return None;
    };
    let channel = |value: &Value| Some(f64::from(u8::try_from(value.as_u64()?).ok()?) / 255.0);
    let opacity = look.get("opacity").and_then(Value::as_f64).unwrap_or(1.0);
    Color::new(channel(r)?, channel(g)?, channel(b)?, opacity)
}

/// Names each face of the body by its persistent name, which a rebuild of
/// the design keeps.
fn name_faces(geometry: &mut Topology, body: &ModelBody<'_>) {
    for (face, name) in geometry.faces.iter_mut().zip(&body.face_names) {
        if !name.is_empty() {
            face.name = Some(name.clone());
        }
    }
}

/// Applies the document's appearances to one body's records: the body's
/// look by its name, and each face look whose persistent name is a face of
/// the body.
fn paint(geometry: &mut Topology, body: &ModelBody<'_>, appearances: Option<&Value>) {
    let Some(appearances) = appearances else {
        return;
    };
    if let Some(name) = &body.name {
        let color = appearances
            .get("bodies")
            .and_then(|bodies| bodies.get(name))
            .and_then(look_color);
        if let (Some(color), Some(record)) = (color, geometry.bodies.first_mut()) {
            record.color = Some(color);
        }
    }
    let Some(looks) = appearances.get("faces").and_then(Value::as_array) else {
        return;
    };
    for look in looks {
        if look.get("body").and_then(Value::as_str) != body.name.as_deref() {
            continue;
        }
        let (Some(name), Some(color)) = (
            look.get("name").and_then(Value::as_str),
            look.get("look").and_then(look_color),
        ) else {
            continue;
        };
        let Some(index) = body.face_names.iter().position(|face| face == name) else {
            continue;
        };
        if let Some(face) = geometry.faces.get_mut(index) {
            face.color = Some(color);
        }
    }
}

/// The body of a mesh slice, whose triangles go to `tessellations`.
fn mesh_body(
    ctx: &DecodeContext<'_>,
    body: &ModelBody<'_>,
    tessellations: &mut Vec<Tessellation>,
) -> Result<Body, CodecError> {
    let read = mesh::read(ctx, body.slice)?;
    let id = BodyId::compose(
        &cadmpeg_ir::identity_namespace!("solidcraft", "cache", "body"),
        body.key.clone(),
    );
    let tessellation = Tessellation::from_parts(
        TessellationId::compose(
            &cadmpeg_ir::identity_namespace!("solidcraft", "cache", "tessellation"),
            body.key.clone(),
        ),
        read.mesh,
        Vec::new(),
    )
    .map_err(|error| CodecError::malformed(format_args!("mesh body: {error}")))?
    .with_body(Some(id.clone()));
    tessellations.push(tessellation);
    Ok(Body {
        id,
        kind: BodyKind::General,
        transform: None,
        name: body.name.clone(),
        color: read.color.and_then(|[r, g, b]| Color::new(r, g, b, 1.0)),
        visible: None,
        native_ref: None,
    })
}

/// The native record of a body slice.
fn body_record(slice: View<'_>) -> Result<brep::Saved, CodecError> {
    if !slice.window().starts_with(value::SIGNATURE) {
        let signature = slice.window().get(..8).unwrap_or_default();
        return Err(CodecError::malformed(format_args!(
            "the slice is a {} body, not a native B-rep",
            String::from_utf8_lossy(signature)
        )));
    }
    let saved: brep::Saved = serde_json::from_value(value::read(slice)?)
        .map_err(|error| CodecError::malformed(format_args!("body slice record: {error}")))?;
    if saved.solid.version != brep::SCHEMA {
        return Err(CodecError::malformed(format_args!(
            "B-rep schema {} is not schema {}",
            saved.solid.version,
            brep::SCHEMA
        )));
    }
    Ok(saved)
}

enum Selection<'a> {
    Bodies {
        bodies: Vec<ModelBody<'a>>,
        states: usize,
    },
    Stale,
}

/// The bodies of the final state, or `Stale` for a cache of another
/// document.
fn select<'a>(
    ctx: &DecodeContext<'a>,
    package: &Package<'a>,
    document: View<'_>,
) -> Result<Selection<'a>, CodecError> {
    let manifest = package.member(ctx, MANIFEST_MEMBER)?;
    let text = std::str::from_utf8(manifest.window())
        .map_err(|_| malformed("the manifest is not UTF-8"))?;
    let manifest = ctx.parse_json_value(text, "SolidCraft model cache")?;
    if manifest.get("format").and_then(Value::as_str) != Some(CACHE_FORMAT) {
        return Err(malformed("the manifest format"));
    }
    if manifest.get("document").and_then(Value::as_str)
        != Some(cadmpeg_ir::hash::sha256_hex(document.window()).as_str())
    {
        return Ok(Selection::Stale);
    }
    let payload = package.member(ctx, PAYLOAD_MEMBER)?;
    if manifest.get("payload").and_then(Value::as_str)
        != Some(cadmpeg_ir::hash::sha256_hex(payload.window()).as_str())
    {
        return Err(malformed("the payload digest"));
    }

    let mut slices = Vec::new();
    let mut end = 0_usize;
    for slice in list(&manifest, "slices")? {
        let offset = count(slice.get(0), "slice offset")?;
        let length = count(slice.get(1), "slice length")?;
        if offset != end {
            return Err(malformed("the slices are not contiguous"));
        }
        end = offset
            .checked_add(length)
            .ok_or_else(|| malformed("slice length"))?;
        slices.push(
            payload
                .child(offset, end)
                .ok_or_else(|| malformed("a slice ends past the payload"))?,
        );
    }
    if end != payload.window().len() {
        return Err(malformed("the payload has bytes after its slices"));
    }
    let table = list(&manifest, "bodies")?;
    let records = manifest.get("model").ok_or_else(|| malformed("model"))?;
    let features = list(records, "features")?;
    let states = list(records, "states")?;
    let model_bodies = list(records, "model_bodies")?;
    let names = records
        .get("names")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let Some(last) = features
        .iter()
        .rev()
        .find(|feature| feature.get("skipped").and_then(Value::as_bool) == Some(false))
    else {
        return Ok(Selection::Bodies {
            bodies: Vec::new(),
            states: states.len(),
        });
    };
    let state = states
        .get(count(last.get("state"), "feature state")?)
        .ok_or_else(|| malformed("feature state index"))?;
    let mut bodies = Vec::new();
    for run in list(state, "bodies")? {
        let first = count(run.get(0), "state body run")?;
        let length = count(run.get(1), "state body run")?;
        let last = first
            .checked_add(length)
            .filter(|last| *last <= model_bodies.len());
        let Some(last) = last else {
            return Err(malformed("a state body run ends past the model bodies"));
        };
        for model_body in &model_bodies[first..last] {
            let entry = table
                .get(count(model_body.get("body"), "model body")?)
                .ok_or_else(|| malformed("model body index"))?;
            let slice = slices
                .get(count(entry.get(0), "body slice")?)
                .copied()
                .ok_or_else(|| malformed("body slice index"))?;
            let key = model_body.get("key").and_then(Value::as_str).map_or_else(
                || IdentityKey::from(bodies.len()),
                IdentityKey::encode_segment,
            );
            bodies.push(ModelBody {
                key,
                name: model_body
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                slice,
                feature: model_body.get("feature").and_then(Value::as_u64),
                component: model_body
                    .get("component")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                face_names: model_body
                    .get("names")
                    .and_then(Value::as_u64)
                    .and_then(|index| usize::try_from(index).ok())
                    .and_then(|index| names.get(index))
                    .and_then(|names| names.get("faces"))
                    .and_then(Value::as_array)
                    .map(|faces| {
                        faces
                            .iter()
                            .map(|face| face.as_str().unwrap_or_default().to_owned())
                            .collect()
                    })
                    .unwrap_or_default(),
            });
        }
    }
    Ok(Selection::Bodies {
        bodies,
        states: states.len(),
    })
}

/// Moves the records of one body into the model as they are: their
/// identities are already scoped by the model body's key.
struct Unchanged;

impl EntityRewrite for Unchanged {
    type Error = CodecError;

    fn rewrite<T: cadmpeg_ir::schema::rewrite::typed::RewriteIdentities>(
        &mut self,
        entity: T,
    ) -> Result<T, CodecError> {
        Ok(entity)
    }
}
