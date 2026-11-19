// SPDX-License-Identifier: Apache-2.0
//! Incremental Keywords XML listing tokens.

use cadmpeg_core::decode::{u64_from_index, View};
use cadmpeg_core::scan::{DesignScan, FeatureCensus, ScanBudget, StopReason, Suppressed};

fn malformed() -> StopReason {
    StopReason::Malformed("SolidWorks history XML framing".into())
}

struct Tokens<'a, 'b> {
    bytes: &'a [u8],
    at: usize,
    budget: &'b ScanBudget<'b>,
}
impl<'a> Tokens<'a, '_> {
    fn byte(&mut self) -> Result<u8, StopReason> {
        self.budget.check()?;
        let value = *self
            .bytes
            .get(self.at)
            .ok_or_else(|| StopReason::Truncated("SolidWorks history XML token".into()))?;
        self.at += 1;
        Ok(value)
    }
    fn whitespace(&mut self) -> Result<(), StopReason> {
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.byte()?;
        }
        Ok(())
    }
    fn name(&mut self) -> Result<&'a str, StopReason> {
        let start = self.at;
        while self
            .bytes
            .get(self.at)
            .is_some_and(|byte| !byte.is_ascii_whitespace() && !b"<>/=?\"'".contains(byte))
        {
            self.byte()?;
        }
        if start == self.at {
            return Err(malformed());
        }
        std::str::from_utf8(&self.bytes[start..self.at]).map_err(|_| malformed())
    }
    fn until(&mut self, marker: &[u8]) -> Result<(), StopReason> {
        loop {
            self.budget.check()?;
            if self
                .bytes
                .get(self.at..)
                .is_some_and(|tail| tail.starts_with(marker))
            {
                self.at += marker.len();
                return Ok(());
            }
            self.byte()?;
        }
    }
    fn value(&mut self) -> Result<String, StopReason> {
        let quote = self.byte()?;
        if !matches!(quote, b'\'' | b'"') {
            return Err(malformed());
        }
        let start = self.at;
        while self.bytes.get(self.at) != Some(&quote) {
            self.byte()?;
        }
        let raw = std::str::from_utf8(&self.bytes[start..self.at]).map_err(|_| malformed())?;
        self.byte()?;
        self.budget.retain(u64_from_index(raw.len()))?;
        let mut value = String::new();
        let mut tail = raw;
        while let Some(at) = tail.find('&') {
            self.budget.check()?;
            value.push_str(&tail[..at]);
            tail = &tail[at + 1..];
            let end = tail.find(';').ok_or_else(malformed)?;
            let entity = &tail[..end];
            let ch = match entity {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                entity => {
                    let number = if let Some(hex) = entity.strip_prefix("#x") {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        entity.strip_prefix('#').and_then(|text| text.parse().ok())
                    };
                    number.and_then(char::from_u32).ok_or_else(malformed)?
                }
            };
            value.push(ch);
            tail = &tail[end + 1..];
        }
        value.push_str(tail);
        Ok(value)
    }
}

pub(super) fn census(
    source: View<'_>,
    budget: &ScanBudget<'_>,
    scan: &mut DesignScan,
) -> Result<bool, StopReason> {
    let mut source = source;
    if source.window().starts_with(&[0x86]) {
        source.req_u8()?;
    }
    let mut converted = String::new();
    let bytes = if source.unread().starts_with(&[0xff, 0xfe]) {
        source.req_take(2)?;
        budget.retain(
            u64_from_index(source.remaining())
                .checked_mul(2)
                .ok_or(StopReason::RetainedBytes)?,
        )?;
        let mut units = Vec::new();
        while source.remaining() != 0 {
            budget.check()?;
            units.push(source.req_u16_le()?);
        }
        for ch in char::decode_utf16(units) {
            budget.check()?;
            converted.push(ch.map_err(|_| malformed())?);
        }
        converted.as_bytes()
    } else {
        source
            .unread()
            .strip_prefix(&[0xef, 0xbb, 0xbf])
            .unwrap_or(source.unread())
    };
    let mut tokens = Tokens {
        bytes,
        at: 0,
        budget,
    };
    let mut stack = Vec::new();
    let mut root_seen = false;
    let mut root_closed = false;
    let mut history = false;
    while tokens.at < bytes.len() {
        budget.check()?;
        let byte = tokens.byte()?;
        if byte != b'<' {
            if stack.is_empty() && !byte.is_ascii_whitespace() {
                return Err(malformed());
            }
            continue;
        }
        if bytes
            .get(tokens.at..)
            .is_some_and(|tail| tail.starts_with(b"!--"))
        {
            tokens.at += 3;
            tokens.until(b"-->")?;
            continue;
        }
        if bytes.get(tokens.at) == Some(&b'?') {
            tokens.at += 1;
            tokens.until(b"?>")?;
            continue;
        }
        if bytes
            .get(tokens.at..)
            .is_some_and(|tail| tail.starts_with(b"![CDATA["))
        {
            tokens.at += 8;
            tokens.until(b"]]>")?;
            continue;
        }
        if bytes.get(tokens.at) == Some(&b'!') {
            return Err(StopReason::Unsupported("SolidWorks XML declaration".into()));
        }
        if bytes.get(tokens.at) == Some(&b'/') {
            tokens.at += 1;
            let name = tokens.name()?;
            tokens.whitespace()?;
            if tokens.byte()? != b'>' || stack.pop() != Some(name) {
                return Err(malformed());
            }
            if stack.is_empty() {
                root_closed = true;
            }
            continue;
        }
        if root_closed {
            return Err(malformed());
        }
        let name = tokens.name()?;
        let depth = u64_from_index(stack.len())
            .checked_add(1)
            .ok_or(StopReason::NestingDepth)?;
        budget.depth(depth)?;
        if !root_seen {
            history = name.contains("Keywords");
            root_seen = true;
            if history {
                scan.census.get_or_insert_with(FeatureCensus::default);
            }
        }
        let visit = if history && !matches!(name, "Keywords" | "Configuration" | "Dimension") {
            Some(budget.feature()?)
        } else {
            None
        };
        let mut kind = None;
        let mut suppressed = Suppressed::Unknown;
        let mut attributes = std::collections::BTreeSet::new();
        let empty;
        loop {
            budget.check()?;
            tokens.whitespace()?;
            match bytes.get(tokens.at) {
                Some(b'>') => {
                    tokens.at += 1;
                    empty = false;
                    break;
                }
                Some(b'/') => {
                    tokens.at += 1;
                    if tokens.byte()? != b'>' {
                        return Err(malformed());
                    }
                    empty = true;
                    break;
                }
                _ => {}
            }
            let key = tokens.name()?;
            budget.retain(64)?;
            if !attributes.insert(key) {
                return Err(malformed());
            }
            tokens.whitespace()?;
            if tokens.byte()? != b'=' {
                return Err(malformed());
            }
            tokens.whitespace()?;
            let value = tokens.value()?;
            if key == "Type" {
                kind = Some(value);
            } else if key == "Suppressed" {
                suppressed = match value.as_str() {
                    "1" | "true" | "True" => Suppressed::Yes,
                    "0" | "false" | "False" => Suppressed::No,
                    _ => Suppressed::Unknown,
                };
            }
        }
        if let Some(visit) = visit {
            scan.record_feature(visit, kind.as_deref().unwrap_or(name), suppressed)?;
        }
        if empty {
            if stack.is_empty() {
                root_closed = true;
            }
        } else {
            budget.retain(16)?;
            stack.push(name);
        }
    }
    if !root_seen || !root_closed || !stack.is_empty() {
        return Err(StopReason::Truncated(
            "SolidWorks history XML document".into(),
        ));
    }
    Ok(history)
}
