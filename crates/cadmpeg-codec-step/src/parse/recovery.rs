// SPDX-License-Identifier: Apache-2.0
//! Bounded exchange statement recovery and DATA dependency closure.

use super::{
    AnchorEntry, BTreeMap, BTreeSet, BoundedExpansions, DecodeContext, ExpansionOmissions,
    ParseDiagnostic, ParseDiagnosticKind, ParseError, Parser, Range, RawRecord, Value,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartSpec};

impl Parser<'_, '_, '_> {
    pub(super) fn diagnostic(
        &mut self,
        offset: usize,
        kind: ParseDiagnosticKind,
        message: std::fmt::Arguments<'_>,
    ) {
        self.diagnostics.push(ParseDiagnostic {
            offset,
            kind,
            message: message.to_string(),
        });
    }

    fn omit_record(
        &mut self,
        span: Range<usize>,
        omitted: &mut Vec<Range<usize>>,
        reason: std::fmt::Arguments<'_>,
    ) {
        self.diagnostic(span.start, ParseDiagnosticKind::RecordOmitted, reason);
        omitted.push(span);
    }

    pub(super) fn recover_header_record(
        &mut self,
        omitted: &mut Vec<Range<usize>>,
    ) -> Result<Option<super::HeaderRecord>, ParseError> {
        let start = self.current_offset();
        let diagnostic_start = self.diagnostics.len();
        let parsed = (|| {
            let name = self.take_name()?;
            let parameters = self.parameter_nesting(Self::parameters_inner)?;
            self.punct(&super::TokenKind::Semicolon)?;
            Ok(super::HeaderRecord {
                name,
                parameters,
                offset: start,
                end: self.previous_end(),
            })
        })();
        match parsed {
            Ok(record) => Ok(Some(record)),
            Err(error @ (ParseError::Resource(_) | ParseError::Lex(_))) => Err(error),
            Err(error @ ParseError::Syntax { .. }) => {
                self.diagnostics.truncate(diagnostic_start);
                let end = statement_end(self.lexer.input(), start, self.lexer.is_draft())?;
                self.diagnostic(
                    start,
                    ParseDiagnosticKind::HeaderMetadataNoncanonical,
                    format_args!(
                        "bounded HEADER statement omitted: {error}; exact source retained"
                    ),
                );
                omitted.push(start..end);
                self.lexer
                    .set_literal_admission(super::LiteralAdmission::Metadata);
                self.lexer.seek(end);
                self.last_end = end;
                self.current = self.lex_next()?;
                Ok(None)
            }
        }
    }

    pub(super) fn recover_data_record(
        &mut self,
        records: &mut BTreeMap<u64, RawRecord>,
        ambiguous: &mut BTreeSet<u64>,
        omitted: &mut Vec<Range<usize>>,
    ) -> Result<(), ParseError> {
        let start = self.current_offset();
        let source_id = self.current.as_ref().and_then(|token| match token.kind {
            super::TokenKind::Instance(id) => Some(id),
            super::TokenKind::ValueInstance(id) if self.lexer.is_draft() => Some(id),
            _ => None,
        });
        let diagnostic_start = self.diagnostics.len();
        let (id, record) = match self.record_attempt() {
            Ok(super::parts::RecordAttempt::Parsed(id, record)) => (id, record),
            Ok(super::parts::RecordAttempt::GivenUp { end }) => {
                self.diagnostics.truncate(diagnostic_start);
                // The shared helper owns the source bytes and sole unreadable
                // loss. This span remains only in the parser's byte accounting.
                omitted.push(start..end);
                self.budget.retain_bytes(
                    super::u64_from_index(size_of::<usize>()),
                    "STEP shared-home accounting index",
                )?;
                self.shared_omitted.insert(start);
                if let Some(id) = source_id {
                    if let Some(previous) = records.remove(&id) {
                        self.omit_record(previous.span, omitted, format_args!("duplicate instance name #{id}; ambiguous instance omitted; exact source retained"));
                    }
                    ambiguous.insert(id);
                    self.dead.insert(id);
                    self.dead_spans.insert(id, start..end);
                    self.dead_keywords
                        .insert(id, statement_keyword(self.lexer.input(), start, end));
                }
                self.lexer
                    .set_literal_admission(super::LiteralAdmission::Required);
                self.lexer.seek(end);
                self.last_end = end;
                self.current = self.lex_next()?;
                return Ok(());
            }
            Err(error @ ParseError::Resource(_)) => return Err(error),
            Err(error @ ParseError::Lex(_)) => return Err(error),
            Err(error @ ParseError::Syntax { .. }) => {
                self.diagnostics.truncate(diagnostic_start);
                let end = statement_end(self.lexer.input(), start, self.lexer.is_draft())?;
                self.omit_record(
                    start..end,
                    omitted,
                    format_args!("bounded DATA record omitted: {error}; exact source retained"),
                );
                if let Some(id) = source_id {
                    if let Some(previous) = records.remove(&id) {
                        self.omit_record(previous.span, omitted, format_args!("duplicate instance name #{id}; ambiguous instance omitted; exact source retained"));
                    }
                    // A readable name in a defective statement cannot give a
                    // later statement permission to select another definition.
                    ambiguous.insert(id);
                }
                self.lexer
                    .set_literal_admission(super::LiteralAdmission::Required);
                self.lexer.seek(end);
                self.last_end = end;
                self.current = self.lex_next()?;
                return Ok(());
            }
        };
        if ambiguous.contains(&id) || records.contains_key(&id) {
            if let Some(previous) = records.remove(&id) {
                self.omit_record(previous.span, omitted, format_args!("duplicate instance name #{id}; ambiguous instance omitted; exact source retained"));
            }
            ambiguous.insert(id);
            self.omit_record(record.span, omitted, format_args!("duplicate instance name #{id}; ambiguous instance omitted; exact source retained"));
            return Ok(());
        }
        records.insert(id, record);
        Ok(())
    }

    /// Removes each bounded anchor and instance from the interpreted graph,
    /// keeps its exact source, and records its name in `omissions`.
    pub(super) fn omit_bounded_expansions(
        &mut self,
        mut bounded: BoundedExpansions,
        anchors: &mut Vec<AnchorEntry>,
        anchor_spans: &mut Vec<Range<usize>>,
        records: &mut BTreeMap<u64, RawRecord>,
        omitted: &mut Vec<Range<usize>>,
        omissions: &mut ExpansionOmissions,
    ) {
        if !bounded.anchors.is_empty() {
            let entries = std::mem::take(anchors).into_iter();
            for (anchor, span) in entries.zip(std::mem::take(anchor_spans)) {
                if let Some(bound) = bounded.anchors.remove(&anchor.name) {
                    self.diagnostic(
                        span.start,
                        ParseDiagnosticKind::ExpansionBounded,
                        format_args!(
                            "anchor <{}> omitted: {bound}; exact source retained",
                            anchor.name
                        ),
                    );
                    omitted.push(span);
                    omissions.anchors.insert(anchor.name);
                } else {
                    anchors.push(anchor);
                    anchor_spans.push(span);
                }
            }
        }
        for (id, bound) in bounded.records {
            if let Some(record) = records.remove(&id) {
                self.diagnostic(
                    record.span.start,
                    ParseDiagnosticKind::ExpansionBounded,
                    format_args!("instance #{id} omitted: {bound}; exact source retained"),
                );
                omitted.push(record.span);
                omissions.records.insert(id);
            }
        }
    }

    pub(super) fn omit_unresolved_records(
        &mut self,
        records: &mut BTreeMap<u64, RawRecord>,
        external: &BTreeSet<u64>,
        external_values: &BTreeSet<u64>,
        omitted: &mut Vec<Range<usize>>,
    ) -> Result<(), ParseError> {
        // On the parts path each dangling dependent is given up as its own
        // part; the shared helper owns its bytes and sole unreadable loss.
        // The legacy path keeps the omission diagnostics below.
        let through_parts = self.parts.is_some() && self.source.is_some();
        let mut pending = Vec::new();
        let mut reasons = BTreeMap::new();
        for (&id, record) in records.iter() {
            let mut missing = None;
            for partial in &record.partials {
                for value in &partial.parameters {
                    visit_references(value, self.budget, &mut |target, value_instance| {
                        if (value_instance && !external_values.contains(&target))
                            || (!value_instance
                                && !records.contains_key(&target)
                                && !external.contains(&target))
                        {
                            missing = Some((target, value_instance));
                        }
                        Ok(())
                    })?;
                }
            }
            if let Some((target, value_instance)) = missing {
                let prefix = if value_instance { '@' } else { '#' };
                if through_parts {
                    reasons.insert(
                        id,
                        format!("unresolved instance reference {prefix}{target}"),
                    );
                } else {
                    self.diagnostic(record.span.start, ParseDiagnosticKind::RecordOmitted, format_args!("instance #{id} omitted: unresolved instance reference {prefix}{target}; exact source retained"));
                }
                pending.push(id);
            }
        }
        if pending.is_empty() {
            return Ok(());
        }
        // Build the reverse graph only when an omission must propagate. Each
        // edge is indexed once; each removed node unlocks its direct users.
        let mut reverse = BTreeMap::<u64, Vec<u64>>::new();

        for (&id, record) in records.iter() {
            for partial in &record.partials {
                for value in &partial.parameters {
                    visit_references(value, self.budget, &mut |target, value_instance| {
                        if !value_instance {
                            reverse.entry(target).or_default().push(id);
                        }
                        Ok(())
                    })?;
                }
            }
        }
        let mut queued = pending
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        while let Some(id) = pending.pop() {
            if let Some(record) = records.remove(&id) {
                let span = record.span.clone();
                if through_parts {
                    let reason = reasons
                        .remove(&id)
                        .unwrap_or_else(|| "unresolved instance reference".to_owned());
                    self.give_up_dependent(&record, reason)?;
                    self.dead.insert(id);
                    self.dead_spans.insert(id, span.clone());
                    self.dead_keywords
                        .insert(id, Some(record.partials.first().name.clone()));
                    self.dead_records.insert(id, record);
                }
                omitted.push(span);
            }
            if let Some(users) = reverse.get(&id) {
                for &user in users {
                    if queued.insert(user) {
                        if let Some(record) = records.get(&user) {
                            if through_parts {
                                reasons.insert(user, format!("depends on given-up instance #{id}"));
                            } else {
                                self.diagnostic(record.span.start, ParseDiagnosticKind::RecordOmitted, format_args!("instance #{user} omitted: depends on omitted instance #{id}; exact source retained"));
                            }
                            pending.push(user);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Give up one dangling dependent as its own part. The dependent parsed,
    /// but its reference does not resolve, so its statement is the part the
    /// decoder cannot read. Nothing is staged, so the part drops nothing.
    fn give_up_dependent(&mut self, record: &RawRecord, reason: String) -> Result<(), ParseError> {
        let (Some(source), Some(parts)) = (self.source, self.parts.as_deref_mut()) else {
            return Self::err_at(
                record.span.start,
                "dependent part has no source view or part output",
            );
        };
        let Some(extent) = source.child(record.span.start, record.span.end) else {
            return Self::err_at(
                record.span.start,
                "dependent part extent escapes its source input",
            );
        };
        let label = if record.partials.len() == 1 {
            "step.simple_entity_instance"
        } else {
            "step.complex_entity_instance"
        };
        let mut spec = PartSpec::new(label).holding([cadmpeg_ir::ArenaClass::Model]);
        if let Some(stream) = self.part_stream.clone() {
            spec = spec.in_stream(stream);
        }
        self.budget
            .decode_part(parts, extent, spec, |_| {
                Err::<(), CodecError>(CodecError::malformed(reason.clone()))
            })
            .map_err(ParseError::from)?;
        // The helper owns the part bytes and sole unreadable loss. This span
        // remains only in the parser's byte accounting.
        self.budget.retain_bytes(
            super::u64_from_index(size_of::<usize>()),
            "STEP shared-home accounting index",
        )?;
        self.shared_omitted.insert(record.span.start);
        Ok(())
    }
}

/// The entity keyword of a failed statement, when the statement states one.
/// A complex instance starts its body with a parenthesis and states no
/// single keyword.
fn statement_keyword(input: &[u8], start: usize, end: usize) -> Option<String> {
    let mut at = start;
    while at < end && input.get(at).is_some_and(|byte| *byte == b'#') {
        at += 1;
    }
    while at < end && input.get(at).is_some_and(|byte| byte.is_ascii_digit()) {
        at += 1;
    }
    while at < end
        && input
            .get(at)
            .is_some_and(|byte| byte.is_ascii_control() || *byte == b' ')
    {
        at += 1;
    }
    if input.get(at) != Some(&b'=') {
        return None;
    }
    at += 1;
    while at < end
        && input
            .get(at)
            .is_some_and(|byte| byte.is_ascii_control() || *byte == b' ')
    {
        at += 1;
    }
    if input.get(at) == Some(&b'(') {
        return None;
    }
    let keyword_start = at;
    while at < end
        && input.get(at).is_some_and(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'+' | b'!' | b'$')
        })
    {
        at += 1;
    }
    if at == keyword_start {
        return None;
    }
    std::str::from_utf8(input.get(keyword_start..at)?)
        .ok()
        .map(str::to_owned)
}

pub(super) fn visit_references(
    value: &Value,
    budget: &DecodeContext<'_>,
    visit: &mut impl FnMut(u64, bool) -> Result<(), ParseError>,
) -> Result<(), ParseError> {
    let _depth = budget.enter_nested("step_reference_visit_depth")?;

    match value {
        Value::Reference(id) => visit(*id, false)?,
        Value::ExternalReference(id) => visit(*id, true)?,
        Value::List(values) => {
            for value in values {
                visit_references(value, budget, visit)?;
            }
        }
        Value::Typed(_, value) => visit_references(value, budget, visit)?,
        _ => {}
    }
    Ok(())
}

/// A semicolon outside a literal or comment bounds a defective statement.
/// An unterminated literal or comment supplies no continuation boundary.
pub(super) fn statement_end(input: &[u8], start: usize, draft: bool) -> Result<usize, ParseError> {
    let mut at = start;
    while let Some(&byte) = input.get(at) {
        if at > start && matches!(byte, b'#' | b'@') {
            let mut next = at + 1;
            let mut digits = false;
            while let Some(&byte) = input.get(next) {
                if !byte.is_ascii_digit() && !byte.is_ascii_control() {
                    break;
                }
                digits |= byte.is_ascii_digit();
                next += 1;
            }
            while input
                .get(next)
                .is_some_and(|byte| byte.is_ascii_control() || *byte == b' ')
            {
                next += 1;
            }
            if digits && input.get(next) == Some(&b'=') {
                return Ok(at);
            }
        }
        if at > start
            && byte.eq_ignore_ascii_case(&b'E')
            && input[..at]
                .iter()
                .rev()
                .find(|byte| !byte.is_ascii_control())
                .is_some_and(|byte| {
                    !byte.is_ascii_alphanumeric() && !matches!(byte, b'_' | b'!' | b'-')
                })
        {
            if let Some(mut end) = crate::lex::match_ignoring_controls(input, at, b"ENDSEC") {
                while input.get(end).is_some_and(u8::is_ascii_control) {
                    end += 1;
                }

                if input
                    .get(end)
                    .is_some_and(|byte| matches!(byte, b' ' | b';'))
                {
                    return Ok(at);
                }
            }
        }
        if byte == b';' {
            return Ok(at + 1);
        }
        let close = match input.get(at..at + 2) {
            Some(b"/*") => Some(b"*/"),
            Some(b"!*") if draft => Some(b"*!"),
            _ => None,
        };
        if let Some(close) = close {
            at += 2;
            while input.get(at..at + 2) != Some(close) {
                if at >= input.len() {
                    return Parser::err_at(
                        start,
                        "statement recovery cannot establish a boundary: unterminated comment",
                    );
                }
                at += 1;
            }
            at += 2;
            continue;
        }
        let delimiter = match byte {
            b'\'' | b'"' => Some(byte),
            b'<' => Some(b'>'),
            _ => None,
        };
        if let Some(delimiter) = delimiter {
            at += 1;
            loop {
                let Some(&byte) = input.get(at) else {
                    return Parser::err_at(
                        start,
                        "statement recovery cannot establish a boundary: unterminated literal",
                    );
                };
                if byte == delimiter {
                    if delimiter == b'\'' {
                        if let Some(end) =
                            crate::lex::match_exact_ignoring_controls(input, at, b"''")
                        {
                            at = end;
                            continue;
                        }
                    }
                    at += 1;
                    break;
                }
                at += 1;
            }
        } else {
            at += 1;
        }
    }
    // EOF is a known boundary for an incomplete record without an open literal.
    Ok(at)
}

#[cfg(test)]
mod tests;
