// SPDX-License-Identifier: Apache-2.0
//! Bounded exchange statement recovery and DATA dependency closure.

use cadmpeg_core::CodecError;

use super::{
    AnchorEntry, BTreeMap, BTreeSet, BoundedExpansions, DecodeContext, ExpansionOmissions,
    GiveUpCause, GivenUpStatement, ParseDiagnostic, ParseDiagnosticKind, ParseError, Parser, Range,
    RawRecord, Value,
};

impl Parser<'_, '_, '_> {
    /// Parses one statement as a part. `Ok(Err(cause))` gives the statement
    /// up: it violates the grammar or exceeded a bound of one part.
    fn statement_part<T>(
        &mut self,
        parse: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<Result<T, GiveUpCause>, ParseError> {
        let budget = self.budget;
        match budget.run_part(|| Ok(parse(self)))? {
            Ok(Ok(value)) => Ok(Ok(value)),
            Ok(Err(error @ ParseError::Syntax { .. })) => {
                Ok(Err(GiveUpCause::Syntax(error.to_string())))
            }
            Ok(Err(error)) => Err(error),
            Err(CodecError::ResourceLimit(limit)) => Ok(Err(GiveUpCause::Resource(limit))),
            Err(error) => Err(ParseError::Resource(error)),
        }
    }

    /// Gives up the statement at `span` and resumes after it.
    fn give_up_statement(
        &mut self,
        kind: &'static str,
        span: Range<usize>,
        cause: GiveUpCause,
        literals: super::LiteralAdmission,
    ) -> Result<usize, ParseError> {
        let end = span.end;
        self.given_up.push(GivenUpStatement {
            kind,
            span,
            cause,
            dropped: Vec::new(),
            lost: Vec::new(),
        });
        self.lexer.set_literal_admission(literals);
        self.lexer.seek(end);
        self.last_end = end;
        self.current = self.lex_next()?;
        Ok(self.given_up.len() - 1)
    }

    /// Drops the record at `span` because it needs given-up statement `part`.
    fn drop_for_part(&mut self, part: usize, span: Range<usize>, omitted: &mut Vec<Range<usize>>) {
        if let Some(statement) = self.given_up.get_mut(part) {
            statement.dropped.push(span.start);
        }
        omitted.push(span);
    }

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
            given_up: None,
        });
    }

    /// Records the omission of the record `id` whose statement is `span`,
    /// which the parser gives up whole.
    fn give_up_record(
        &mut self,
        id: String,
        span: &Range<usize>,
        kind: ParseDiagnosticKind,
        message: std::fmt::Arguments<'_>,
    ) {
        self.diagnostics.push(ParseDiagnostic {
            offset: span.start,
            kind,
            message: message.to_string(),
            given_up: Some((id, span.len())),
        });
    }

    pub(super) fn recover_header_record(
        &mut self,
    ) -> Result<Option<super::HeaderRecord>, ParseError> {
        let start = self.current_offset();
        let diagnostic_start = self.diagnostics.len();
        let parsed = self.statement_part(|parser| {
            let name = parser.take_name()?;
            let parameters = parser.parameter_nesting(Self::parameters_inner)?;
            parser.punct(&super::TokenKind::Semicolon)?;
            Ok(super::HeaderRecord {
                name,
                parameters,
                offset: start,
                end: parser.previous_end(),
            })
        })?;
        match parsed {
            Ok(record) => Ok(Some(record)),
            Err(cause) => {
                self.diagnostics.truncate(diagnostic_start);
                let end = statement_end(self.lexer.input(), start, self.lexer.is_draft())?;
                self.give_up_statement(
                    "header_record",
                    start..end,
                    cause,
                    super::LiteralAdmission::Metadata,
                )?;
                Ok(None)
            }
        }
    }

    pub(super) fn recover_data_record(
        &mut self,
        records: &mut BTreeMap<u64, RawRecord>,
        stated: &mut BTreeSet<u64>,
        omitted: &mut Vec<Range<usize>>,
    ) -> Result<(), ParseError> {
        let start = self.current_offset();
        let source_id = self.current.as_ref().and_then(|token| match token.kind {
            super::TokenKind::Instance(id) => Some(id),
            super::TokenKind::ValueInstance(id) if self.lexer.is_draft() => Some(id),
            _ => None,
        });
        let diagnostic_start = self.diagnostics.len();
        let (id, record) = match self.statement_part(Self::record)? {
            Ok(record) => record,
            Err(cause) => {
                self.diagnostics.truncate(diagnostic_start);
                let end = statement_end(self.lexer.input(), start, self.lexer.is_draft())?;
                let part = self.give_up_statement(
                    "data_record",
                    start..end,
                    cause,
                    super::LiteralAdmission::Required,
                )?;
                // The first statement of a name stands. A defective first
                // statement gives the name up with it, so no later statement
                // of the name is chosen in its place.
                if let Some(id) = source_id {
                    if stated.insert(id) {
                        self.given_up_names.insert(id, part);
                    }
                }
                return Ok(());
            }
        };
        if !stated.insert(id) {
            let span = record.span;
            let message = format!(
                "repeated instance name #{id}: the first statement stands; the copy at bytes {}..{} is not stated; exact source retained",
                span.start, span.end
            );
            // A copy equal to the statement that stands drops nothing.
            if records
                .get(&id)
                .is_some_and(|first| first.partials == record.partials)
            {
                self.diagnostic(
                    span.start,
                    ParseDiagnosticKind::RecordOmitted,
                    format_args!("{message}"),
                );
            } else {
                self.give_up_record(
                    format!("#{id}"),
                    &span,
                    ParseDiagnosticKind::RecordOmitted,
                    format_args!("{message}"),
                );
            }
            omitted.push(span);
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
                    self.give_up_record(
                        format!("<{}>", anchor.name),
                        &span,
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
                self.give_up_record(
                    format!("#{id}"),
                    &record.span,
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
        // Each pending record carries the given-up statement it needs, if its
        // only missing references name one. A record that names a given-up
        // statement only as entries of unordered aggregates is not pending:
        // it loses those entries. An entry of a positional aggregate, or of
        // a set whose classification carries meaning (a face's bounds, a
        // solid's voids), is needed like a direct reference (§3.3).
        let mut pending = Vec::new();
        let mut listing = BTreeSet::new();
        for (&id, record) in records.iter() {
            let mut missing = None;
            let mut given_up = None;
            let mut listed_given_up = false;
            visit_record_references(
                record,
                self.budget,
                &mut |target, value_instance, unordered| {
                    if (value_instance && !external_values.contains(&target))
                        || (!value_instance
                            && !records.contains_key(&target)
                            && !external.contains(&target))
                    {
                        match self.given_up_names.get(&target) {
                            Some(_) if !value_instance && unordered => listed_given_up = true,
                            Some(&part) if !value_instance => given_up = Some(part),
                            _ => missing = Some((target, value_instance)),
                        }
                    }
                    Ok(())
                },
            )?;
            if let Some((target, value_instance)) = missing {
                let prefix = if value_instance { '@' } else { '#' };
                self.give_up_record(format!("#{id}"), &record.span, ParseDiagnosticKind::RecordOmitted, format_args!("instance #{id} omitted: unresolved instance reference {prefix}{target}; exact source retained"));
                pending.push((id, None));
            } else if given_up.is_some() {
                pending.push((id, given_up));
            } else if listed_given_up {
                listing.insert(id);
            }
        }
        let mut dropped_for = BTreeMap::new();
        if !pending.is_empty() {
            self.propagate_omissions(records, pending, &mut listing, &mut dropped_for, omitted)?;
        }
        for id in listing {
            let Some(mut record) = records.remove(&id) else {
                continue;
            };
            let mut lost = Vec::new();
            drop_unordered_entries(
                &mut record,
                |target| {
                    if records.contains_key(&target) || external.contains(&target) {
                        return None;
                    }
                    self.given_up_names
                        .get(&target)
                        .or_else(|| dropped_for.get(&target))
                        .copied()
                },
                &mut lost,
            );
            records.insert(id, record);
            for (part, entry) in lost {
                if let Some(statement) = self.given_up.get_mut(part) {
                    statement.lost.push((id, entry));
                }
            }
        }
        Ok(())
    }

    /// Removes each pending record and, transitively, every record that
    /// needs a removed one, and maps each record removed for a given-up
    /// statement to that statement in `dropped_for`. A record that names a
    /// record dropped for a given-up statement only as entries of unordered
    /// aggregates is added to `listing` instead.
    fn propagate_omissions(
        &mut self,
        records: &mut BTreeMap<u64, RawRecord>,
        mut pending: Vec<(u64, Option<usize>)>,
        listing: &mut BTreeSet<u64>,
        dropped_for: &mut BTreeMap<u64, usize>,
        omitted: &mut Vec<Range<usize>>,
    ) -> Result<(), ParseError> {
        // Each edge is indexed once; each removed node unlocks its direct
        // users.
        let mut reverse = BTreeMap::<u64, Vec<(u64, bool)>>::new();

        for (&id, record) in records.iter() {
            visit_record_references(
                record,
                self.budget,
                &mut |target, value_instance, unordered| {
                    if !value_instance {
                        reverse.entry(target).or_default().push((id, unordered));
                    }
                    Ok(())
                },
            )?;
        }
        let mut queued = pending.iter().map(|&(id, _)| id).collect::<BTreeSet<_>>();
        while let Some((id, part)) = pending.pop() {
            if let Some(record) = records.remove(&id) {
                match part {
                    Some(part) => {
                        dropped_for.insert(id, part);
                        self.drop_for_part(part, record.span, omitted);
                    }
                    None => omitted.push(record.span),
                }
            }
            if let Some(users) = reverse.get(&id) {
                for &(user, unordered) in users {
                    if part.is_some() && unordered {
                        listing.insert(user);
                        continue;
                    }
                    if queued.insert(user) {
                        if let Some(record) = records.get(&user) {
                            if part.is_none() {
                                self.give_up_record(format!("#{user}"), &record.span, ParseDiagnosticKind::RecordOmitted, format_args!("instance #{user} omitted: depends on omitted instance #{id}; exact source retained"));
                            }
                            pending.push((user, part));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// Visits each reference in `value` with whether it is a value instance.
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

/// Visits each reference of `record` with whether it is a value instance and
/// whether it is an entry of an unordered aggregate attribute.
fn visit_record_references(
    record: &RawRecord,
    budget: &DecodeContext<'_>,
    visit: &mut impl FnMut(u64, bool, bool) -> Result<(), ParseError>,
) -> Result<(), ParseError> {
    let simple = record.partials.len() == 1;
    for partial in &record.partials {
        for (index, value) in partial.parameters.iter().enumerate() {
            match value {
                Value::List(entries) if unordered_attribute(&partial.name, index, simple) => {
                    let _depth = budget.enter_nested("step_reference_visit_depth")?;
                    for entry in entries {
                        match untyped(entry) {
                            Value::Reference(id) => visit(*id, false, true)?,
                            Value::ExternalReference(id) => visit(*id, true, true)?,
                            _ => visit_references(entry, budget, &mut |id, value_instance| {
                                visit(id, value_instance, false)
                            })?,
                        }
                    }
                }
                _ => visit_references(value, budget, &mut |id, value_instance| {
                    visit(id, value_instance, false)
                })?,
            }
        }
    }
    Ok(())
}

/// Removes each entry of an unordered aggregate attribute of `record` that
/// refers to a record `removed` maps to a given-up statement, and pushes the
/// statement and the entry's instance name to `lost`.
fn drop_unordered_entries(
    record: &mut RawRecord,
    removed: impl Fn(u64) -> Option<usize>,
    lost: &mut Vec<(usize, u64)>,
) {
    let simple = record.partials.len() == 1;
    for partial in &mut record.partials {
        let name = partial.name.as_str();
        for (index, value) in (&mut partial.parameters).into_iter().enumerate() {
            let Value::List(entries) = value else {
                continue;
            };
            if !unordered_attribute(name, index, simple) {
                continue;
            }
            entries.retain(|entry| match untyped(entry) {
                Value::Reference(id) => match removed(*id) {
                    Some(part) => {
                        lost.push((part, *id));
                        false
                    }
                    None => true,
                },
                _ => true,
            });
        }
    }
}

/// The value under any defined-type wrappers.
fn untyped(mut value: &Value) -> &Value {
    while let Value::Typed(_, inner) = value {
        value = inner;
    }
    value
}

/// SET and BAG attributes in the external mapping of a simple instance
/// whose entries carry no meaning by classification.
const UNORDERED_SIMPLE: &[(&str, usize)] = &[
    ("ADVANCED_BREP_SHAPE_REPRESENTATION", 1),
    ("CLOSED_SHELL", 1),
    ("CONNECTED_FACE_SET", 1),
    ("CONSTRUCTIVE_GEOMETRY_REPRESENTATION", 1),
    ("DEFINITIONAL_REPRESENTATION", 1),
    ("DERIVED_UNIT", 0),
    ("DRAUGHTING_MODEL", 1),
    ("EDGE_BASED_WIREFRAME_SHAPE_REPRESENTATION", 1),
    ("FACETED_BREP_SHAPE_REPRESENTATION", 1),
    ("FACE_BASED_SURFACE_MODEL", 1),
    ("FILL_AREA_STYLE", 1),
    ("GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION", 1),
    ("GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION", 1),
    ("GEOMETRIC_CURVE_SET", 1),
    ("GEOMETRIC_SET", 1),
    ("GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT", 2),
    ("GLOBAL_UNIT_ASSIGNED_CONTEXT", 2),
    ("MANIFOLD_SURFACE_SHAPE_REPRESENTATION", 1),
    ("MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION", 1),
    ("OPEN_SHELL", 1),
    ("OVER_RIDING_STYLED_ITEM", 1),
    ("PRESENTATION_LAYER_ASSIGNMENT", 2),
    ("PRESENTATION_STYLE_ASSIGNMENT", 0),
    ("PRODUCT", 3),
    ("PRODUCT_RELATED_PRODUCT_CATEGORY", 2),
    ("REPRESENTATION", 1),
    ("SHAPE_REPRESENTATION", 1),
    ("SHELL_BASED_SURFACE_MODEL", 1),
    ("SHELL_BASED_WIREFRAME_SHAPE_REPRESENTATION", 1),
    ("STYLED_ITEM", 1),
    ("SURFACE_SIDE_STYLE", 1),
    ("TESSELLATED_SHAPE_REPRESENTATION", 1),
    ("TESSELLATED_SHELL", 1),
    ("TESSELLATED_SOLID", 1),
];

/// SET and BAG attributes among the own attributes of a complex partial
/// whose entries carry no meaning by classification.
const UNORDERED_PARTIAL: &[(&str, usize)] = &[
    ("CONNECTED_FACE_SET", 0),
    ("DERIVED_UNIT", 0),
    ("FACE_BASED_SURFACE_MODEL", 0),
    ("FILL_AREA_STYLE", 1),
    ("GEOMETRIC_SET", 0),
    ("GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT", 0),
    ("GLOBAL_UNIT_ASSIGNED_CONTEXT", 0),
    ("PRESENTATION_LAYER_ASSIGNMENT", 2),
    ("PRESENTATION_STYLE_ASSIGNMENT", 0),
    ("PRODUCT", 3),
    ("PRODUCT_RELATED_PRODUCT_CATEGORY", 0),
    ("REPRESENTATION", 1),
    ("SHELL_BASED_SURFACE_MODEL", 0),
    ("STYLED_ITEM", 0),
    ("SURFACE_SIDE_STYLE", 1),
    ("TESSELLATED_SHELL", 0),
    ("TESSELLATED_SOLID", 0),
];

/// Whether parameter `index` of a partial record named `entity` is a SET or
/// BAG attribute in the integrated resources of ISO 10303 (parts 41, 42, 43
/// and 46) whose entries carry no meaning by classification. `simple`
/// selects the external mapping of a simple instance, whose parameters
/// include inherited attributes; a partial of a complex instance holds its
/// entity's own attributes only. Every other aggregate is needed whole: a
/// positional one would shift its later entries, and a face without one of
/// its bounds or a solid without one of its voids is another face or solid.
fn unordered_attribute(entity: &str, index: usize, simple: bool) -> bool {
    let table = if simple {
        UNORDERED_SIMPLE
    } else {
        UNORDERED_PARTIAL
    };
    table
        .binary_search_by(|&(name, _)| name.cmp(entity))
        .is_ok_and(|at| table[at].1 == index)
}

/// A semicolon outside a literal or comment bounds a defective statement.
/// An unterminated literal or comment supplies no continuation boundary.
fn statement_end(input: &[u8], start: usize, draft: bool) -> Result<usize, ParseError> {
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
