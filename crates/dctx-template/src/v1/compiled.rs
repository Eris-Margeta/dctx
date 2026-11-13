// SPDX-License-Identifier: Apache-2.0
//! V1 compiled template and its `DCTXTMPL` byte encoding.
//! Implements ADR-003 ahead-of-time compilation. The magic and field order match dctx-core.
//! Depends on [`super::parse`] and [`crate::error`]. Callers compile before render or plan lowering.
//! Encoding is in memory only: this crate does not open files.

use std::io::{self, Read, Write};

use super::Template;
use super::parse::replace_vars_bounded;
use crate::error::TemplateValidationError;

const MAGIC: &[u8; 8] = b"DCTXTMPL";
const VERSION: u32 = 1;

/// A v1 template with `{{var}}` already substituted into the partials and layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledTemplate {
    /// Caller-supplied name.
    pub name: String,
    /// Resolved variables.
    pub variables: std::collections::HashMap<String, String>,
    /// Tree partial after substitution.
    pub partial_tree_entry: String,
    /// File partial after substitution.
    pub partial_file_block: String,
    /// Layout after substitution.
    pub layout: String,
}

impl Template {
    /// Validates and substitutes `{{var}}` into the partials and layout.
    ///
    /// # Errors
    /// Returns validation errors for reference cycles, unresolved variables or expansion above
    /// the variable and 1 MiB substituted-body limits. Oversized substitutions are refused before allocation.
    pub fn compile(&self, name: &str) -> Result<CompiledTemplate, Vec<TemplateValidationError>> {
        let errors = self.validate();
        if !errors.is_empty() {
            return Err(errors);
        }
        let resolve = |text: &str| {
            replace_vars_bounded(text, &self.variables)
                .map_err(|error| vec![TemplateValidationError::InvalidSyntax(error.to_string())])
        };
        Ok(CompiledTemplate {
            name: name.to_string(),
            variables: self.variables.clone(),
            partial_tree_entry: resolve(&self.partial_tree_entry)?,
            partial_file_block: resolve(&self.partial_file_block)?,
            layout: resolve(&self.layout)?,
        })
    }
}

impl CompiledTemplate {
    /// Writes magic `DCTXTMPL`, version 1, then name, variables, partials and layout.
    /// Variables are encoded in lexical key order. Names are at most 65,535 UTF-8 bytes;
    /// other strings and the variable count use unsigned 32-bit lengths.
    /// The writer may contain a partial template on error; discard that partial output.
    ///
    /// # Errors
    /// Refuses fields too long for their encoded lengths and propagates writer IO failures.
    pub fn serialize<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(MAGIC)?;
        writer.write_all(&VERSION.to_le_bytes())?;
        write_u32_bytes(writer, self.name.as_bytes())?;
        let mut keys: Vec<&String> = self.variables.keys().collect();
        keys.sort();
        let count = u32::try_from(keys.len())
            .map_err(|_| invalid("too many variables; reduce the variable count to u32::MAX"))?;
        writer.write_all(&count.to_le_bytes())?;
        for key in keys {
            let Some(val) = self.variables.get(key) else {
                continue;
            };
            let key_len = u16::try_from(key.len())
                .map_err(|_| invalid("variable name too long; shorten it to 65535 UTF-8 bytes"))?;
            writer.write_all(&key_len.to_le_bytes())?;
            writer.write_all(key.as_bytes())?;
            write_u32_bytes(writer, val.as_bytes())?;
        }
        write_u32_bytes(writer, self.partial_tree_entry.as_bytes())?;
        write_u32_bytes(writer, self.partial_file_block.as_bytes())?;
        write_u32_bytes(writer, self.layout.as_bytes())?;
        Ok(())
    }

    /// Reads one template written by [`CompiledTemplate::serialize`], leaving trailing input unread.
    /// This legacy format has no aggregate size cap; bound the reader when accepting external data.
    ///
    /// # Errors
    /// Refuses invalid magic, versions, duplicate variables, lengths or UTF-8 and propagates incomplete-read/reader IO failures.
    /// Lengths are read incrementally: an incomplete field never preallocates its claimed size.
    pub fn deserialize<R: Read>(reader: &mut R) -> io::Result<Self> {
        let mut magic = [0u8; 8];
        reader.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(invalid(
                "invalid template magic; supply a serialized DCTXTMPL template",
            ));
        }
        let version = read_u32(reader)?;
        if version != VERSION {
            return Err(invalid(
                "unsupported template version; recompile as DCTXTMPL version 1",
            ));
        }
        let name = read_string(reader)?;
        let var_count = read_u32(reader)?;
        let mut variables = std::collections::HashMap::new();
        for _ in 0..var_count {
            let key_len = read_u16(reader)?;
            let key = read_string_n(reader, key_len)?;
            let val = read_string(reader)?;
            if variables.insert(key, val).is_some() {
                return Err(invalid("duplicate variable; keep one definition per name"));
            }
        }
        let partial_tree_entry = read_string(reader)?;
        let partial_file_block = read_string(reader)?;
        let layout = read_string(reader)?;
        Ok(Self {
            name,
            variables,
            partial_tree_entry,
            partial_file_block,
            layout,
        })
    }

    /// Rebuilds a [`Template`] whose partials are already substituted.
    /// No validation is performed and the cycle list is empty; validate before further compilation.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn to_template(&self) -> Template {
        Template {
            variables: self.variables.clone(),
            partial_tree_entry: self.partial_tree_entry.clone(),
            partial_file_block: self.partial_file_block.clone(),
            layout: self.layout.clone(),
            cycles: Vec::new(),
        }
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn write_u32_bytes<W: Write>(writer: &mut W, bytes: &[u8]) -> io::Result<()> {
    let len = u32::try_from(bytes.len())
        .map_err(|_| invalid("field too long; shorten the field to u32::MAX bytes"))?;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(bytes)?;
    Ok(())
}

fn read_u32<R: Read>(reader: &mut R) -> io::Result<u32> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u16<R: Read>(reader: &mut R) -> io::Result<usize> {
    let mut buf = [0u8; 2];
    reader.read_exact(&mut buf)?;
    Ok(usize::from(u16::from_le_bytes(buf)))
}

fn read_string_n<R: Read>(reader: &mut R, len: usize) -> io::Result<String> {
    let mut bytes = Vec::new();
    reader.take(len as u64).read_to_end(&mut bytes)?;
    if bytes.len() != len {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "truncated template field; supply the complete serialized template",
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| invalid("invalid utf-8; encode template strings as UTF-8 and serialize again"))
}

fn read_string<R: Read>(reader: &mut R) -> io::Result<String> {
    let len = usize::try_from(read_u32(reader)?).map_err(|_| {
        invalid("length is not addressable; shorten the field or use a wider target")
    })?;
    read_string_n(reader, len)
}
