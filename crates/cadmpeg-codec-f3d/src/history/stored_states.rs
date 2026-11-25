// SPDX-License-Identifier: Apache-2.0
//! The stored states of one ASM history as topology records, for the design
//! states of a timeline (§5.4).
//!
//! The walk from the head undoes the changes of each `delta_state` in turn,
//! so each visit holds the record table of one stored state. A state's
//! records are those its table decodes to, as the model blob is decoded,
//! confined to the bodies of the current geometry and the bodies the head
//! does not hold that are no sketch region.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_core::decode::{DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::{CadIr, Topology};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::units::Tolerances;

use crate::brep::Brep;

/// The record work, in table entries, that the stored states of one decode
/// may spend.
pub(crate) const MAX_STATE_WORK: usize = 1 << 22;

/// The model blob of one ASM history.
pub(crate) struct Blob<'a> {
    pub(crate) bytes: &'a [u8],
    /// The entry name of the blob.
    pub(crate) stream: &'a str,
    pub(crate) width: RefWidth,
    /// The namespace that qualifies the blob's record ids, where the decode
    /// selects several model blobs.
    pub(crate) namespace: Option<&'a str>,
}

/// The states of one history that a timeline's states use.
pub(crate) struct Read {
    /// The state id of the head.
    pub(crate) head: i64,
    /// The records of the head that the current bodies reach.
    pub(crate) head_records: Topology,
    /// Each wanted state other than the head, newest first, with its records,
    /// or why they are not read.
    pub(crate) states: Vec<(i64, Result<Topology, String>)>,
}

/// The states `wanted` of the history of `blob`, whose head holds the
/// `current` bodies. The outer error is a decode ceiling. The inner error
/// says why the history gives no states.
pub(crate) fn read(
    ctx: &DecodeContext<'_>,
    blob: &Blob<'_>,
    current: &BTreeSet<BodyId>,
    wanted: &BTreeSet<i64>,
    tolerances: &Tolerances,
    work: &WorkBudget,
) -> Result<Result<Read, String>, CodecError> {
    let Some(header) = cadmpeg_asm::asm_header::parse(blob.bytes) else {
        return Ok(Err("the blob has no ASM header".to_owned()));
    };
    let Some((states, archive)) = super::decode_unbound(ctx, blob.bytes, blob.stream, blob.width)?
    else {
        return Ok(Err(
            "the history or its record archive does not decode".to_owned()
        ));
    };
    let Some(active_count) = super::archived_active_record_count(&states)?
        .or_else(|| super::insert_only_active_record_count(&states))
        .and_then(|count| i64::try_from(count).ok())
    else {
        return Ok(Err("the active record count is not stated".to_owned()));
    };
    let archived_ids = states
        .iter()
        .flat_map(|state| &state.records)
        .filter_map(|record| record.revision_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let subtypes =
        super::history_bodies::archive_subtypes(&archive, header.metadata.save_format_version);
    let decoder = StateDecoder {
        ctx,
        blob,
        header: &header.metadata,
        subtypes: &subtypes,
        archive: &archive,
        tolerances,
        work,
    };
    let mut head = None::<Result<(i64, BTreeSet<BodyId>, Topology), String>>;
    let mut read = Vec::new();
    let mut stopped = None::<String>;
    let mut failure = None::<CodecError>;
    let complete = super::walk_entity_versions_hashed(
        &states,
        active_count,
        &archived_ids,
        |index, versions| {
            if failure.is_some() {
                return;
            }
            let state_id = states[index].state_id;
            let outcome = match &head {
                None => decoder.head(versions, current).map(|found| {
                    head = Some(found.map(|(bodies, records)| (state_id, bodies, records)));
                }),
                Some(Err(_)) => Ok(()),
                Some(Ok(_)) if !wanted.contains(&state_id) => Ok(()),
                Some(Ok((_, head_bodies, _))) => match &stopped {
                    Some(reason) => {
                        read.push((state_id, Err(reason.clone())));
                        Ok(())
                    }
                    None => decoder
                        .state(versions, current, head_bodies)
                        .map(|records| {
                            if let Err(reason) = &records {
                                stopped = Some(format!("a later state is not read ({reason})"));
                            }
                            read.push((state_id, records));
                        }),
                },
            };
            if let Err(error) = outcome {
                failure = Some(error);
            }
        },
    );
    if let Some(error) = failure {
        return Err(error);
    }
    if !complete {
        return Ok(Err("the walk of its states is incomplete".to_owned()));
    }
    match head {
        Some(Ok((head, _, head_records))) => Ok(Ok(Read {
            head,
            head_records,
            states: read,
        })),
        Some(Err(reason)) => Ok(Err(reason)),
        None => Ok(Err("the history stores no state".to_owned())),
    }
}

/// What decodes the record table of one state of a history.
struct StateDecoder<'a, 'c> {
    ctx: &'a DecodeContext<'c>,
    blob: &'a Blob<'a>,
    header: &'a cadmpeg_asm::kernel_header::KernelHeader,
    subtypes: &'a cadmpeg_asm::nurbs::toks::SubtypeTable,
    archive: &'a super::HistoricalRecordArchive,
    tolerances: &'a Tolerances,
    work: &'a WorkBudget,
}

impl StateDecoder<'_, '_> {
    /// The bodies of the head and the records the `current` bodies reach,
    /// or why they are not read.
    fn head(
        &self,
        versions: &super::binding::FastMap<i64, i64>,
        current: &BTreeSet<BodyId>,
    ) -> Result<Result<(BTreeSet<BodyId>, Topology), String>, CodecError> {
        let brep = match self.brep(versions)? {
            Ok(brep) => brep,
            Err(reason) => return Ok(Err(format!("its head is not read ({reason})"))),
        };
        let bodies = brep
            .asm
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect::<BTreeSet<_>>();
        if let Some(body) = current.iter().find(|body| !bodies.contains(*body)) {
            return Ok(Err(format!("its head holds no body {body}")));
        }
        Ok(self
            .records(brep, current.iter().cloned().collect())?
            .map(|records| (bodies, records)))
    }

    /// The records of one state that is not the head: those its `current`
    /// bodies reach, and those of each body the head does not hold that is
    /// no sketch region.
    fn state(
        &self,
        versions: &super::binding::FastMap<i64, i64>,
        current: &BTreeSet<BodyId>,
        head_bodies: &BTreeSet<BodyId>,
    ) -> Result<Result<Topology, String>, CodecError> {
        let brep = match self.brep(versions)? {
            Ok(brep) => brep,
            Err(reason) => return Ok(Err(reason)),
        };
        let regions = brep.sketch_region_bodies();
        let kept = brep
            .asm
            .bodies
            .iter()
            .map(|body| &body.id)
            .filter(|body| {
                current.contains(*body) || !(head_bodies.contains(*body) || regions.contains(*body))
            })
            .cloned()
            .collect();
        self.records(brep, kept)
    }

    /// The B-rep graph of the record table `versions` maps to, with the ids
    /// of the model blob.
    fn brep(
        &self,
        versions: &super::binding::FastMap<i64, i64>,
    ) -> Result<Result<Brep, String>, CodecError> {
        if !self.work.charge_by(versions.len()) {
            return Ok(Err("the state work bound is reached".to_owned()));
        }
        let Some(table) = super::history_bodies::materialize_hashed_table(versions, self.archive)
        else {
            return Ok(Err("its record table is incomplete".to_owned()));
        };
        let mut brep = match crate::brep::decode_state(
            self.ctx,
            &table,
            self.blob.bytes,
            (self.header, self.subtypes),
            self.blob.stream,
        ) {
            Ok(brep) => brep,
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => return Ok(Err(format!("its record table does not decode: {error}"))),
        };
        if let Some(namespace) = self.blob.namespace {
            brep.qualify_ids(self.ctx, crate::ids::ID_FORMAT, namespace)?;
        }
        Ok(Ok(brep))
    }

    /// The topology records of `brep` that the bodies `kept` reach, as the
    /// model geometry admits them.
    fn records(
        &self,
        mut brep: Brep,
        kept: Vec<BodyId>,
    ) -> Result<Result<Topology, String>, CodecError> {
        brep.retain_bodies(self.ctx, kept)?;
        let mut scratch = CadIr::empty();
        scratch.tolerances = *self.tolerances;
        let remainder = cadmpeg_asm::brep::transfer::transfer_into_ir(
            self.ctx,
            &mut scratch,
            cadmpeg_ir::codec_id!("f3d"),
            brep.asm,
        )
        .map(|(_, remainder)| remainder);
        let admitted = remainder.and_then(|mut remainder| {
            crate::decode::admit_state_geometry(
                self.ctx,
                &mut scratch,
                &mut remainder.annotation_records,
            )
        });
        match admitted {
            Ok(()) => Ok(Ok(scratch.geometry)),
            Err(error @ CodecError::ResourceLimit(_)) => Err(error),
            Err(error) => Ok(Err(format!("its records do not transfer: {error}"))),
        }
    }
}

/// The state id of each `delta_state` of the history of `blob` and the
/// native id of its record, read from the state headers.
pub(crate) fn state_ids(blob: &Blob<'_>) -> Option<BTreeMap<i64, String>> {
    let history = super::decode_header_index(blob.bytes, blob.stream, blob.width)?;
    Some(
        history
            .states()
            .iter()
            .map(|state| (state.state_id, state.id.clone()))
            .collect(),
    )
}
