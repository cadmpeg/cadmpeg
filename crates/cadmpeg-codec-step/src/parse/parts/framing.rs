// SPDX-License-Identifier: Apache-2.0
//! Source-only entity boundaries and actual complex outer parentheses.

/// A boundary observed without parsing or rebuilding a record's values.
#[derive(Clone, Copy)]
pub(super) enum Boundary {
    Terminator(usize),
    Parent(usize),
    EndOfInput(usize),
}
impl Boundary {
    pub(super) fn end(self) -> usize {
        match self {
            Self::Terminator(end) | Self::Parent(end) | Self::EndOfInput(end) => end,
        }
    }
    pub(super) fn terminated(self) -> bool {
        matches!(self, Self::Terminator(_))
    }
}

pub(super) fn boundary(
    input: &[u8],
    start: usize,
    draft: bool,
) -> Result<Option<Boundary>, super::ParseError> {
    match super::super::recovery::statement_end(input, start, draft) {
        Ok(end) if end.checked_sub(1).and_then(|last| input.get(last)) == Some(&b';') => {
            Ok(Some(Boundary::Terminator(end)))
        }
        Ok(end) if end == input.len() => Ok(Some(Boundary::EndOfInput(end))),
        Ok(end) => Ok(
            crate::lex::match_ignoring_controls(input, end, b"ENDSEC").and_then(|after| {
                input
                    .get(after)
                    .filter(|byte| byte.is_ascii_control() || matches!(**byte, b' ' | b';'))
                    .map(|_| Boundary::Parent(end))
            }),
        ),
        // The scanner's syntax failures are unfinished shielded constructs.
        // Their bytes run to the genuine input boundary; apparent delimiters
        // and assignments inside them are never siblings.
        Err(super::ParseError::Syntax { .. }) => Ok(Some(Boundary::EndOfInput(input.len()))),
        Err(error) => Err(error),
    }
}

pub(super) fn outer_close(input: &[u8], open: usize, end: usize, draft: bool) -> Option<usize> {
    let mut at = open.checked_add(1)?;
    let mut depth = 1_usize;
    while at < end {
        let byte = *input.get(at)?;
        let pair = input.get(at..at.checked_add(2)?);
        let comment = if pair == Some(b"/*") {
            Some(b"*/".as_slice())
        } else if draft && pair == Some(b"!*") {
            Some(b"*!".as_slice())
        } else {
            None
        };
        if let Some(close) = comment {
            let content = at.checked_add(2)?;
            let length = input
                .get(content..end)?
                .windows(2)
                .position(|pair| pair == close)?;
            at = content.checked_add(length)?.checked_add(2)?;
            continue;
        }
        let delimiter = match byte {
            b'\'' | b'"' => Some(byte),
            b'<' => Some(b'>'),
            _ => None,
        };
        if let Some(delimiter) = delimiter {
            at = at.checked_add(1)?;
            loop {
                if at >= end {
                    return None;
                }
                if *input.get(at)? == delimiter {
                    if delimiter == b'\'' {
                        if let Some(next) =
                            crate::lex::match_exact_ignoring_controls(input, at, b"''")
                                .filter(|next| *next <= end)
                        {
                            at = next;
                            continue;
                        }
                    }
                    at = at.checked_add(1)?;
                    break;
                }
                at = at.checked_add(1)?;
            }
            continue;
        }
        match byte {
            b'(' => depth = depth.checked_add(1)?,
            b')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
        at = at.checked_add(1)?;
    }
    None
}
