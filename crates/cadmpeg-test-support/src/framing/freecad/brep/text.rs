// SPDX-License-Identifier: Apache-2.0
//! Text table and record locators, independent of the codec's parser.
use super::grammar::{self, Layout};
use super::*;
struct Token<'a> {
    bytes: &'a [u8],
    start: usize,
    end: usize,
}
struct Cursor<'a, 'b> {
    tokens: &'b [Token<'a>],
    at: usize,
}
impl<'a> Cursor<'a, '_> {
    fn token(&mut self) -> Option<&'a [u8]> {
        let value = self.tokens.get(self.at)?;
        self.at += 1;
        Some(value.bytes)
    }
}
impl Layout for Cursor<'_, '_> {
    fn kind(&mut self) -> Option<usize> {
        std::str::from_utf8(self.token()?).ok()?.parse().ok()
    }
    fn degree(&mut self) -> Option<usize> {
        self.count().filter(|value| *value <= 64)
    }
    fn count(&mut self) -> Option<usize> {
        self.kind().filter(|value| *value <= 1_000_000)
    }
    fn signed(&mut self) -> Option<()> {
        std::str::from_utf8(self.token()?)
            .ok()?
            .parse::<i64>()
            .ok()
            .map(|_| ())
    }
    fn reals(&mut self, n: usize) -> Option<()> {
        for _ in 0..n {
            let value = std::str::from_utf8(self.token()?)
                .ok()?
                .parse::<f64>()
                .ok()?;
            if !value.is_finite() {
                return None;
            }
        }
        Some(())
    }
    fn knots(&mut self, n: usize) -> Option<()> {
        for _ in 0..n {
            self.reals(1)?;
            self.count()?;
        }
        Some(())
    }
}
fn tokens<'a>(ctx: &DecodeContext<'_>, source: View<'a>) -> Result<Vec<Token<'a>>, String> {
    let mut out = Vec::new();
    let bytes = source.window();
    let mut at = 0;
    let mut count_follows = false;
    while at < bytes.len() {
        if bytes[at].is_ascii_whitespace() {
            at += 1;
            continue;
        }
        let start = at;
        if let Some(width) = marker_width(&bytes[start..]) {
            at += width;
            count_follows = true;
        } else if count_follows && bytes[start].is_ascii_digit() {
            while at < bytes.len() && bytes[at].is_ascii_digit() {
                at += 1;
            }
            count_follows = false;
        } else if bytes[start] == b'*' {
            at += 1;
        } else {
            count_follows = false;
            while at < bytes.len()
                && !bytes[at].is_ascii_whitespace()
                && bytes[at] != b'*'
                && (at == start || marker_width(&bytes[at..]).is_none())
            {
                at += 1;
            }
        }
        ctx.retain_bytes(
            u64_from_index(size_of::<Token<'_>>()),
            "B-rep token boundaries",
        )
        .map_err(|e| e.to_string())?;
        out.try_reserve(1).map_err(|e| e.to_string())?;
        out.push(Token {
            bytes: &bytes[start..at],
            start: source.start() + start,
            end: source.start() + at,
        });
    }
    Ok(out)
}
fn marker_width(bytes: &[u8]) -> Option<usize> {
    TABLES.iter().find_map(|name| {
        let suffix = bytes.strip_prefix(*name)?;
        suffix
            .first()
            .is_none_or(u8::is_ascii_whitespace)
            .then_some(name.len())
    })
}
pub(super) fn frame<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    root: usize,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    let Some((hs, he)) = header(source.window()) else {
        return Ok(());
    };
    let absolute = source.start();
    let line = &source.window()[hs..he];
    let prefix = b"CASCADE Topology V";
    let version = line
        .strip_prefix(prefix)
        .and_then(|tail| tail.first())
        .and_then(|byte| byte.checked_sub(b'0'));
    let mut framing = Vec::new();
    let mut dispatch = Vec::new();
    if line.last() == Some(&b'\n') {
        framing.push(range(absolute + he - 1, absolute + he));
    }
    if version.is_some() {
        let digit = absolute + hs + prefix.len();
        framing.push(range(digit, digit + 1));
        dispatch.push(range(digit, digit + 1));
        framing.sort_by_key(|r| r.start);
    }
    add(
        ctx,
        source,
        absolute + hs,
        absolute + he,
        "freecad.brep_text_header",
        root,
        framing,
        dispatch,
        out,
    )?;
    let Some(version @ 1..=3) = version else {
        return Ok(());
    };
    let tokens = tokens(ctx, source)?;
    let mut markers = Vec::new();
    let mut prior = 0;
    for name in TABLES {
        let positions = tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| token.bytes == name)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if positions.len() != 1 || positions[0] < prior {
            return Ok(());
        }
        let position = positions[0];
        prior = position + 1;
        markers.push(position);
    }
    let mut root_start = None;
    for (table, position) in markers.iter().copied().enumerate() {
        let start = tokens[position].start;
        let marker_end = tokens
            .get(position + 1)
            .map_or(tokens[position].end, |token| token.end);
        let count = tokens
            .get(position + 1)
            .and_then(|token| std::str::from_utf8(token.bytes).ok()?.parse::<usize>().ok())
            .filter(|n| *n <= 1_000_000);
        let framing_end = count.map_or(tokens[position].end, |_| marker_end);
        let begin = position + 2;
        let mut end_index = markers.get(table + 1).copied().unwrap_or(tokens.len());
        if table == 7 {
            if let Some(count) = count {
                let mut found = 0;
                end_index = begin;
                while end_index < tokens.len() && found < count {
                    if tokens[end_index].bytes == b"*" {
                        found += 1;
                    }
                    end_index += 1;
                }
                if found == count {
                    root_start = Some(end_index);
                }
            }
        }
        let end = if table == 7 && root_start.is_some() {
            end_index
                .checked_sub(1)
                .and_then(|index| tokens.get(index))
                .map_or(marker_end, |token| token.end)
        } else {
            tokens
                .get(end_index)
                .map_or(source.end(), |token| token.start)
        };
        let parent = add(
            ctx,
            source,
            start,
            end,
            "freecad.brep_text_table",
            root,
            vec![range(start, framing_end)],
            vec![range(start, tokens[position].end)],
            out,
        )?;
        let Some(count) = count else { continue };
        let Some(body) = tokens.get(begin..end_index) else {
            continue;
        };
        let mut c = Cursor {
            tokens: body,
            at: 0,
        };
        for _ in 0..count {
            let row_begin = c.at;
            let row_start = body.get(row_begin).map_or(end, |token| token.start);
            if table == 7 {
                let terminator = body[c.at..]
                    .iter()
                    .position(|token| token.bytes == b"*")
                    .map(|offset| c.at + offset);
                let row_end = terminator.map_or(end, |index| body[index].end);
                let flags = terminator.and_then(|limit| flags(&body[row_begin..limit], version));
                let mut dispatch = body
                    .get(row_begin)
                    .map(|token| vec![range(token.start, token.end)])
                    .unwrap_or_default();
                if let Some(flags) = flags {
                    dispatch.push(range(flags.start, flags.end));
                    dispatch.sort_by_key(|r| r.start);
                }
                add(
                    ctx,
                    source,
                    row_start,
                    row_end,
                    TEXT_ROWS[table],
                    parent,
                    terminator
                        .map(|index| vec![range(body[index].start, body[index].end)])
                        .unwrap_or_default(),
                    dispatch,
                    out,
                )?;
                let Some(index) = terminator else { break };
                c.at = index + 1;
                continue;
            }
            let success = row(&mut c, table, version).is_some();
            let row_end = if success {
                c.at.checked_sub(1)
                    .and_then(|index| body.get(index))
                    .map_or(row_start, |token| token.end)
            } else {
                end
            };
            let mut dispatch = Vec::new();
            let selected = match table {
                0 | 1 | 2 | 5 => Some(row_begin),
                3 => Some(row_begin + 1),
                4 => {
                    let mut probe = Cursor {
                        tokens: body,
                        at: row_begin,
                    };
                    probe
                        .count()
                        .and_then(|n| row_begin.checked_add(n)?.checked_add(3))
                }
                6 => Some(row_begin + 2),
                _ => None,
            };
            if let Some(index) = selected.and_then(|index| body.get(index)) {
                if index.end <= row_end {
                    dispatch.push(range(index.start, index.end));
                }
            }
            if table == 6 && version >= 3 {
                if let Some(token) = body.get(row_begin + 3) {
                    if token.end <= row_end {
                        dispatch.push(range(token.start, token.end));
                    }
                }
            }
            add(
                ctx,
                source,
                row_start,
                row_end,
                TEXT_ROWS[table],
                parent,
                vec![],
                dispatch,
                out,
            )?;
            if !success {
                break;
            }
        }
    }
    if let Some(index) = root_start.filter(|index| *index < tokens.len()) {
        let start = index
            .checked_sub(1)
            .and_then(|i| tokens.get(i))
            .map_or(source.start(), |token| token.end);
        let framing = tokens[index..]
            .iter()
            .filter(|token| token.bytes == b"*")
            .map(|token| range(token.start, token.end))
            .collect();
        add(
            ctx,
            source,
            start,
            source.end(),
            "freecad.brep_text_root",
            root,
            framing,
            vec![],
            out,
        )?;
    }
    Ok(())
}
fn row(c: &mut Cursor<'_, '_>, table: usize, version: u8) -> Option<()> {
    match table {
        0 => grammar::location(c),
        1 => grammar::curve(c, 2, 0),
        2 => grammar::curve(c, 3, 0),
        5 => grammar::surface(c, 0),
        3 => {
            let n = c.count()?;
            let parameters = c.flag()?;
            c.reals(1 + n.checked_mul(3 + usize::from(parameters))?)
        }
        4 => {
            let n = c.count()?;
            for _ in 0..n {
                c.count()?;
            }
            if c.token()? != b"p" {
                return None;
            }
            c.reals(1)?;
            if c.flag()? {
                c.reals(n)?;
            }
            Some(())
        }
        6 => {
            let n = c.count()?;
            let triangles = c.count()?;
            let uv = c.flag()?;
            let normals = version >= 3 && c.flag()?;
            c.reals(1 + n.checked_mul(3 + 2 * usize::from(uv))?)?;
            for _ in 0..triangles.checked_mul(3)? {
                c.count()?;
            }
            if normals {
                c.reals(n.checked_mul(3)?)?;
            }
            Some(())
        }
        _ => None,
    }
}

fn flags<'a, 'b>(tokens: &'b [Token<'a>], version: u8) -> Option<&'b Token<'a>> {
    let mut c = Cursor { tokens, at: 0 };
    match c.token()? {
        b"Ve" => {
            c.reals(4)?;
            loop {
                c.reals(1)?;
                match c.kind()? {
                    0 => break,
                    1 => {
                        c.count()?;
                        c.count()?;
                    }
                    2 => {
                        for _ in 0..3 {
                            c.count()?;
                        }
                    }
                    3 => {
                        c.reals(1)?;
                        c.count()?;
                        c.count()?;
                    }
                    _ => return None,
                }
            }
        }
        b"Ed" => {
            c.reals(1)?;
            c.flag()?;
            c.flag()?;
            c.flag()?;
            loop {
                match c.kind()? {
                    0 => break,
                    1 => {
                        c.count()?;
                        c.count()?;
                        c.reals(2)?;
                    }
                    2 | 3 => {
                        let kind = std::str::from_utf8(c.tokens.get(c.at.checked_sub(1)?)?.bytes)
                            .ok()?
                            .parse::<usize>()
                            .ok()?;
                        c.count()?;
                        if kind == 3 {
                            let secondary = c.token()?;
                            let digits = secondary
                                .iter()
                                .take_while(|byte| byte.is_ascii_digit())
                                .count();
                            if digits == 0 {
                                return None;
                            }
                            if digits == secondary.len() {
                                c.token()?;
                            }
                        }
                        c.count()?;
                        c.count()?;
                        c.reals(2)?;
                        if version == 2 {
                            c.reals(4)?;
                        }
                    }
                    4 => {
                        c.token()?;
                        for _ in 0..4 {
                            c.count()?;
                        }
                    }
                    5 => {
                        c.count()?;
                        c.count()?;
                    }
                    6 => {
                        for _ in 0..3 {
                            c.count()?;
                        }
                    }
                    7 => {
                        for _ in 0..4 {
                            c.count()?;
                        }
                    }
                    _ => return None,
                }
            }
        }
        b"Fa" => {
            c.flag()?;
            c.reals(1)?;
            c.count()?;
            c.count()?;
            if c.tokens.get(c.at).is_some_and(|token| token.bytes == b"2") {
                c.kind()?;
                c.count()?;
            }
        }
        b"Wi" | b"Sh" | b"So" | b"CS" | b"Co" => {}
        _ => return None,
    }
    tokens.get(c.at).filter(|token| {
        token.bytes.len() == 7 && token.bytes.iter().all(|byte| matches!(byte, b'0' | b'1'))
    })
}
