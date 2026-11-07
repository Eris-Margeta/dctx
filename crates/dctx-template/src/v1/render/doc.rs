// SPDX-License-Identifier: Apache-2.0
//! Run-length piece table for the bounded marker renderer.
//! Substitution and trimming stay below the render ceiling without allocating oversized documents.
//! Depends on [`super::parse`]. Used by [`super::measure`].

use super::*;
use crate::TemplateError;

pub(super) fn too_big() -> TemplateError {
    TemplateError::BudgetTooSmall {
        minimum_bytes: u64::MAX,
    }
}

#[derive(Clone)]
struct Piece {
    text: String,
    times: u64,
}

#[derive(Clone, Default)]
pub(super) struct Doc {
    pieces: Vec<Piece>,
}

impl Doc {
    pub(super) fn len(&self) -> Result<u64, TemplateError> {
        let mut total = 0u64;
        for piece in &self.pieces {
            let one = u64::try_from(piece.text.len()).map_err(|_| too_big())?;
            let part = one.checked_mul(piece.times).ok_or_else(too_big)?;
            total = total.checked_add(part).ok_or_else(too_big)?;
        }
        Ok(total)
    }

    pub(super) fn from_str(text: &str) -> Self {
        Self::from_string(text.to_string())
    }

    pub(super) fn from_string(text: String) -> Self {
        if text.is_empty() {
            return Self::default();
        }
        Self {
            pieces: vec![Piece { text, times: 1 }],
        }
    }

    fn push_times(&mut self, text: &str, times: u64) -> Result<(), TemplateError> {
        if times == 0 || text.is_empty() {
            return Ok(());
        }
        if let Some(last) = self.pieces.last_mut()
            && last.text == text
        {
            last.times = last.times.checked_add(times).ok_or_else(too_big)?;
            return Ok(());
        }
        self.pieces.push(Piece {
            text: text.to_string(),
            times,
        });
        Ok(())
    }

    pub(super) fn push_str(&mut self, text: &str) -> Result<(), TemplateError> {
        self.push_times(text, 1)
    }

    pub(super) fn push_repeated(
        &mut self,
        replacement: &Doc,
        times: u64,
    ) -> Result<(), TemplateError> {
        if times == 0 || replacement.pieces.is_empty() {
            return Ok(());
        }
        if replacement.pieces.len() == 1 {
            let piece = &replacement.pieces[0];
            let total = piece.times.checked_mul(times).ok_or_else(too_big)?;
            return self.push_times(&piece.text, total);
        }
        let unit = replacement.len()?;
        if times > 1 && unit.saturating_mul(times) > RENDER_CEILING {
            return Err(too_big());
        }
        for _ in 0..times {
            for piece in &replacement.pieces {
                self.push_times(&piece.text, piece.times)?;
            }
        }
        Ok(())
    }

    pub(super) fn materialize(&self) -> Result<String, TemplateError> {
        let len = self.len()?;
        if len > RENDER_CEILING {
            return Err(too_big());
        }
        let mut out = String::new();
        if let Ok(bytes) = usize::try_from(len) {
            out.reserve(bytes);
        }
        for piece in &self.pieces {
            for _ in 0..piece.times {
                out.push_str(&piece.text);
            }
        }
        Ok(out)
    }

    pub(super) fn interpolate(text: &str, fields: &[(&str, &Doc)]) -> Result<Doc, TemplateError> {
        let mut output = Doc::default();
        let mut parts = data_parts(text).peekable();
        while let Some(part) = parts.next() {
            if let Some((_, value)) = fields.iter().find(|(field, _)| *field == part) {
                let mut times = 1u64;
                while parts.peek() == Some(&part) {
                    parts.next();
                    times = times.checked_add(1).ok_or_else(too_big)?;
                }
                output.push_repeated(value, times)?;
            } else {
                output.push_str(part)?;
            }
        }
        Ok(output)
    }

    pub(super) fn trimmed(&self) -> Result<Doc, TemplateError> {
        if self.len()? <= RENDER_CEILING {
            let text = self.materialize()?;
            return Ok(Doc::from_string(text.trim().to_string()));
        }
        Ok(self.trim_large())
    }

    fn trim_large(&self) -> Doc {
        let mut pieces = self.pieces.clone();
        while pieces
            .first()
            .is_some_and(|piece| whitespace_unit(&piece.text))
        {
            pieces.remove(0);
        }
        while pieces
            .last()
            .is_some_and(|piece| whitespace_unit(&piece.text))
        {
            pieces.pop();
        }
        if pieces.is_empty() {
            return Doc::default();
        }
        trim_start_side(&mut pieces);
        trim_end_side(&mut pieces);
        pieces.retain(|piece| piece.times > 0 && !piece.text.is_empty());
        Doc { pieces }
    }
}

fn whitespace_unit(text: &str) -> bool {
    text.chars().all(char::is_whitespace)
}

fn trim_start_side(pieces: &mut Vec<Piece>) {
    if pieces.is_empty() {
        return;
    }
    let trimmed = pieces[0].text.trim_start().to_string();
    if trimmed.len() == pieces[0].text.len() {
        return;
    }
    let times = pieces[0].times;
    if times == 1 {
        if trimmed.is_empty() {
            pieces.remove(0);
        } else {
            pieces[0].text = trimmed;
        }
        return;
    }
    let original = pieces[0].text.clone();
    let mut rebuilt = Vec::new();
    if !trimmed.is_empty() {
        rebuilt.push(Piece {
            text: trimmed,
            times: 1,
        });
    }
    rebuilt.push(Piece {
        text: original,
        times: times - 1,
    });
    rebuilt.extend(pieces.drain(1..));
    *pieces = rebuilt;
}

fn trim_end_side(pieces: &mut Vec<Piece>) {
    if pieces.is_empty() {
        return;
    }
    let index = pieces.len() - 1;
    let trimmed = pieces[index].text.trim_end().to_string();
    if trimmed.len() == pieces[index].text.len() {
        return;
    }
    let times = pieces[index].times;
    if times == 1 {
        if trimmed.is_empty() {
            pieces.pop();
        } else {
            pieces[index].text = trimmed;
        }
        return;
    }
    pieces[index].times = times - 1;
    if !trimmed.is_empty() {
        pieces.push(Piece {
            text: trimmed,
            times: 1,
        });
    }
}
