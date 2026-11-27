// SPDX-License-Identifier: Apache-2.0
//! Contain errors at a bounded Design record.

use crate::container::ContainerScan;
use crate::loss::F3dLossCode;
use cadmpeg_core::decode::{u64_from_index, ArenaClass, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::{DecodeParts, GivenUpPart, GivenUpRecord, Part};
use cadmpeg_ir::{SourceProvenance, StreamName};

/// Whether `error` ends the session rather than one record: an I/O error or a
/// resource refusal.
pub(crate) fn is_fatal(error: &CodecError) -> bool {
    matches!(error, CodecError::Io(_) | CodecError::ResourceLimit(_))
}

/// A Design record that a give-up names without its frame in hand: the
/// record with record index `entity` in the archive stream `stream`, at its
/// start offset in that stream where the caller holds it. Its id is
/// `f3d:<stream>:design-entity#<entity>`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DesignRecord<'a> {
    stream: &'a str,
    entity: u64,
    offset: Option<usize>,
}

impl<'a> DesignRecord<'a> {
    /// The record with record index `entity` in `stream`.
    pub(crate) fn entity(stream: &'a str, entity: impl Into<u64>) -> Self {
        Self {
            stream,
            entity: entity.into(),
            offset: None,
        }
    }

    /// The record starts at byte `offset` of its stream.
    pub(crate) const fn at(mut self, offset: usize) -> Self {
        self.offset = Some(offset);
        self
    }

    /// The record's id token.
    pub(crate) fn id(&self) -> String {
        crate::ids::native_scoped_id(self.stream, "design-entity", self.entity)
    }

    /// The losses of the record given up whole for the codec loss `why`:
    /// `why` at the record's position, then the `record.unreadable` that
    /// names the record by its id.
    pub(crate) fn given_up(&self, why: LossNote) -> Result<[LossNote; 2], CodecError> {
        let why = with_position(why, self.stream, self.offset);
        let unreadable =
            GivenUpRecord::caused_by(cadmpeg_ir::ids::Identity::new(self.id())?, &why).loss();
        Ok([why, unreadable])
    }
}

fn with_position(why: LossNote, stream: &str, offset: Option<usize>) -> LossNote {
    let provenance = StreamName::try_from(stream.to_owned())
        .ok()
        .zip(offset)
        .map(|(stream, offset)| SourceProvenance::in_stream("f3d", stream, u64_from_index(offset)));
    match provenance {
        Some(provenance) => why.with_provenance(provenance),
        None => why,
    }
}

/// The part kind of a Design record given up with its frame.
pub(crate) const DESIGN_RECORD_PART: &str = "design-record";

/// The frame of a Design record: bytes `start..end` of the archive stream
/// `stream`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RecordFrame<'a> {
    stream: &'a str,
    start: usize,
    end: usize,
}

impl<'a> RecordFrame<'a> {
    pub(crate) const fn new(stream: &'a str, start: usize, end: usize) -> Self {
        Self { stream, start, end }
    }

    /// Gives the record up whole for the codec loss `why`: `why` goes to
    /// `losses` at the record's position, and the frame goes to `frames`
    /// for the caller to give up as a part.
    pub(crate) fn give_up(
        self,
        why: LossNote,
        losses: &mut Vec<LossNote>,
        frames: &mut Vec<GivenUpFrame>,
    ) {
        let why = with_position(why, self.stream, Some(self.start));
        frames.push(self.caused_by(&why));
        losses.push(why);
    }

    /// The frame, given up for the codec loss `why` that the caller reports
    /// once for every frame it names.
    pub(crate) fn caused_by(self, why: &LossNote) -> GivenUpFrame {
        GivenUpFrame {
            stream: self.stream.to_owned(),
            start: self.start,
            end: self.end,
            error: format!("{}: {}", why.code, why.message),
        }
    }
}

/// A Design record given up whole with its frame, which a decode pass that
/// holds no decode context collects for its caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GivenUpFrame {
    stream: String,
    start: usize,
    end: usize,
    error: String,
}

impl GivenUpFrame {
    /// The byte range of the frame in its stream.
    #[cfg(test)]
    pub(crate) fn span(&self) -> (&str, std::ops::Range<usize>) {
        (&self.stream, self.start..self.end)
    }

    /// Gives the frame up as a `design-record` part that keeps its bytes.
    /// The inflated stream bytes are retained in no other record.
    pub(crate) fn give_up(
        self,
        ctx: &DecodeContext<'_>,
        scan: &ContainerScan<'_>,
    ) -> Result<GivenUpPart, CodecError> {
        let bytes = scan
            .entry_bytes(&self.stream)?
            .get(self.start..self.end)
            .ok_or_else(|| CodecError::malformed("F3D Design record frame escapes its stream"))?;
        let stream = StreamName::try_from(self.stream).map_err(CodecError::malformed)?;
        let part = Part::at(
            "f3d",
            DESIGN_RECORD_PART,
            u64_from_index(self.start),
            bytes,
            [ArenaClass::Design],
        )
        .in_stream(stream);
        ctx.give_up_part(part, CodecError::Malformed(self.error))
    }
}

/// Gives up each frame in `frames` as a part. Frames that start at one
/// offset of one stream are one part, whose stub id the stream and offset
/// mint: the part spans the longest of them and its loss states every cause,
/// in the order the passes gave them.
pub(crate) fn give_up_frames(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    frames: impl IntoIterator<Item = GivenUpFrame>,
) -> Result<Vec<GivenUpPart>, CodecError> {
    let mut merged: Vec<GivenUpFrame> = Vec::new();
    let mut at = std::collections::HashMap::new();
    for frame in frames {
        match at.entry((frame.stream.clone(), frame.start)) {
            std::collections::hash_map::Entry::Occupied(slot) => {
                let kept: &mut GivenUpFrame = &mut merged[*slot.get()];
                kept.end = kept.end.max(frame.end);
                if !kept.error.contains(&frame.error) {
                    kept.error.push_str("; ");
                    kept.error.push_str(&frame.error);
                }
            }
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(merged.len());
                merged.push(frame);
            }
        }
    }
    merged
        .into_iter()
        .map(|frame| frame.give_up(ctx, scan))
        .collect()
}

/// Contains an error at one record the caller then gives up. An I/O error or
/// a resource refusal is returned to the caller and ends the session. Every
/// other error becomes `code`'s loss, whose message describes the record as
/// `what`, and one `record.unreadable` naming `record` by its id, and the
/// record stays in the source archive; the caller then decodes the next
/// record.
pub(crate) fn recover<T>(
    result: Result<T, CodecError>,
    losses: &mut Vec<LossNote>,
    code: F3dLossCode,
    record: DesignRecord<'_>,
    what: impl std::fmt::Display,
) -> Result<Option<T>, CodecError> {
    match result {
        Ok(record) => Ok(Some(record)),
        Err(error) if is_fatal(&error) => Err(error),
        Err(error) => {
            losses.extend(record.given_up(failure_note(code, what, &error))?);
            Ok(None)
        }
    }
}

/// Contains an error at one record whose frame the caller holds, as
/// [`recover`] does, and gives the record up with [`RecordFrame::give_up`].
pub(crate) fn recover_frame<T>(
    result: Result<T, CodecError>,
    losses: &mut Vec<LossNote>,
    frames: &mut Vec<GivenUpFrame>,
    code: F3dLossCode,
    frame: RecordFrame<'_>,
    what: impl std::fmt::Display,
) -> Result<Option<T>, CodecError> {
    match result {
        Ok(record) => Ok(Some(record)),
        Err(error) if is_fatal(&error) => Err(error),
        Err(error) => {
            frame.give_up(failure_note(code, what, &error), losses, frames);
            Ok(None)
        }
    }
}

/// Contains an error in a member of a record the caller keeps with less
/// content, as [`recover`] does, charging `code`'s loss alone.
pub(crate) fn recover_partial<T>(
    result: Result<T, CodecError>,
    losses: &mut Vec<LossNote>,
    code: F3dLossCode,
    what: impl std::fmt::Display,
) -> Result<Option<T>, CodecError> {
    match result {
        Ok(record) => Ok(Some(record)),
        Err(error) if is_fatal(&error) => Err(error),
        Err(error) => {
            losses.push(failure_note(code, what, &error));
            Ok(None)
        }
    }
}

fn failure_note(code: F3dLossCode, what: impl std::fmt::Display, error: &CodecError) -> LossNote {
    code.note(format!(
        "{what} remains in the source archive because its decoding failed: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::{recover, recover_frame, recover_partial, DesignRecord, RecordFrame};
    use crate::loss::F3dLossCode;
    use cadmpeg_core::decode::{LimitScope, ResourceDimension, ResourceFailure, ResourceLimit};
    use cadmpeg_core::CodecError;

    const STREAM: &str = "Design/BulkStream.dat";

    #[test]
    fn record_recovery_preserves_resource_refusals() {
        let limit = ResourceLimit {
            scope: LimitScope::for_dimension(ResourceDimension::RecursionDepth),
            dimension: ResourceDimension::RecursionDepth,
            reason: ResourceFailure::BudgetExceeded,
            limit: 1,
            used: 1,
            additional: 1,
            operation: "synthetic record recursion",
        };
        let mut losses = Vec::new();
        let result = recover::<()>(
            Err(CodecError::ResourceLimit(limit)),
            &mut losses,
            F3dLossCode::SketchPointUndecoded,
            DesignRecord::entity(STREAM, 42_u64),
            "Sketch point record 42",
        );
        assert!(matches!(result, Err(CodecError::ResourceLimit(_))));
        assert!(losses.is_empty());
    }

    #[test]
    fn a_given_up_record_charges_its_code_and_record_unreadable() {
        let mut losses = Vec::new();
        let result = recover::<()>(
            Err(CodecError::malformed("synthetic point failure")),
            &mut losses,
            F3dLossCode::SketchPointUndecoded,
            DesignRecord::entity(STREAM, 42_u64).at(100),
            "Sketch point record 42 version 11 in Design/BulkStream.dat",
        );
        assert!(matches!(result, Ok(None)));
        assert_eq!(
            crate::test_support::loss_codes(&losses),
            crate::test_support::given_up_codes(F3dLossCode::SketchPointUndecoded)
        );
        assert!(losses[0].message.contains("synthetic point failure"));
        assert_eq!(losses[1].severity, cadmpeg_ir::report::Severity::Error);
        assert!(losses[1].message.starts_with(&format!(
            "record f3d:{STREAM}:design-entity#42 stream=\"{STREAM}\" offset=100 given up: {}: ",
            F3dLossCode::SketchPointUndecoded.kind()
        )));
        assert!(losses[1].message.ends_with("; dropped=[]"));
        assert_eq!(losses[0].provenance, losses[1].provenance);
        assert_eq!(losses[1].provenance.as_ref().map(|at| at.offset), Some(100));
    }

    /// §3.3: a record whose frame the decoder holds is a part. The site
    /// charges its code alone and hands the frame to the part helper.
    #[test]
    fn a_framed_record_charges_its_code_and_hands_over_its_frame() {
        let mut losses = Vec::new();
        let mut frames = Vec::new();
        let result = recover_frame::<()>(
            Err(CodecError::malformed("synthetic point failure")),
            &mut losses,
            &mut frames,
            F3dLossCode::SketchPointUndecoded,
            RecordFrame::new(STREAM, 100, 160),
            "Sketch point record 42 version 11 in Design/BulkStream.dat",
        );
        assert!(matches!(result, Ok(None)));
        assert_eq!(
            crate::test_support::loss_codes(&losses),
            [F3dLossCode::SketchPointUndecoded.kind()]
        );
        assert_eq!(losses[0].provenance.as_ref().map(|at| at.offset), Some(100));
        let [frame] = frames.as_slice() else {
            panic!("one given-up frame: {frames:?}");
        };
        assert_eq!(frame.span(), (STREAM, 100..160));
    }

    /// Two passes that give up one frame start mint one part id; the part
    /// spans the longer frame and names both causes.
    #[test]
    fn two_frames_at_one_start_give_up_one_part_naming_both_causes() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        const DESIGN: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
        let archive =
            crate::test_support::body_map_test::f3d_with_breps_and_design(&[], &[0x5a; 64], &[]);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&archive, &arena, &policy).unwrap();
        let scan = crate::container::scan(&ctx, root).unwrap();
        let mut losses = Vec::new();
        let mut frames = Vec::new();
        RecordFrame::new(DESIGN, 8, 24).give_up(
            F3dLossCode::SketchPointUndecoded.note("first cause".to_owned()),
            &mut losses,
            &mut frames,
        );
        RecordFrame::new(DESIGN, 8, 40).give_up(
            F3dLossCode::SketchCurveUndecoded.note("second cause".to_owned()),
            &mut losses,
            &mut frames,
        );
        let parts = super::give_up_frames(&ctx, &scan, frames).unwrap();
        let [part] = parts.as_slice() else {
            panic!("one part for one frame start: {}", parts.len());
        };
        let loss = part.loss();
        assert!(
            loss.message.starts_with(&format!(
                "part design-record stream=\"{DESIGN}\" offset=8 length=32 "
            )),
            "{}",
            loss.message
        );
        assert!(
            loss.message.contains("first cause") && loss.message.contains("second cause"),
            "{}",
            loss.message
        );
    }

    #[test]
    fn a_partial_record_charges_its_code_alone() {
        let mut losses = Vec::new();
        let result = recover_partial::<()>(
            Err(CodecError::malformed("synthetic geometry failure")),
            &mut losses,
            F3dLossCode::SketchCurveUndecoded,
            "Sketch curve record 41 in Design/BulkStream.dat",
        );
        assert!(matches!(result, Ok(None)));
        assert_eq!(
            crate::test_support::loss_codes(&losses),
            [F3dLossCode::SketchCurveUndecoded.kind()]
        );
    }

    #[test]
    fn record_recovery_keeps_io_errors_fatal() {
        let mut losses = Vec::new();
        let result = recover::<()>(
            Err(CodecError::Io(std::io::Error::other("read failed"))),
            &mut losses,
            F3dLossCode::SketchPointUndecoded,
            DesignRecord::entity(STREAM, 42_u64),
            "Sketch point record 42",
        );
        assert!(matches!(result, Err(CodecError::Io(_))));
        assert!(losses.is_empty());
    }
}
