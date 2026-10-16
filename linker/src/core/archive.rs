//! Bounded ordinary Unix ar reader. Thin archives and dynamic inputs are rejected.
use object::{ObjectError, ObjectFile, ObjectReadLimits, ObjectReadUsage, ObjectResource};

#[derive(Debug)]
pub struct ArchiveMember<'a> {
    pub name: String,
    pub bytes: &'a [u8],
}

pub fn read_archive(bytes: &[u8]) -> Result<Vec<ArchiveMember<'_>>, String> {
    parse_archive(bytes, 1024 * 1024, 512 * 1024 * 1024).map(|(members, _, _)| members)
}

fn charge_archive(work: &mut usize, amount: usize, limit: usize) -> Result<(), String> {
    *work = work
        .checked_add(amount)
        .filter(|n| *n <= limit)
        .ok_or("archive parsing work budget exceeded")?;
    Ok(())
}

fn parse_archive(
    bytes: &[u8],
    max_names: usize,
    max_work: usize,
) -> Result<(Vec<ArchiveMember<'_>>, usize, usize), String> {
    if !bytes.starts_with(b"!<arch>\n") {
        return Err("expected ordinary ar archive (thin archives are unsupported)".into());
    }
    if bytes.len() > 256 * 1024 * 1024 {
        return Err("archive input byte limit exceeded".into());
    }
    let mut work = 0;
    charge_archive(&mut work, 8, max_work)?;
    let mut cursor = 8usize;
    let mut members = Vec::new();
    let mut names = 0usize;
    let mut long_names = None;
    while cursor < bytes.len() {
        charge_archive(&mut work, 60, max_work)?;
        let header = bytes
            .get(cursor..cursor.checked_add(60).ok_or("archive header overflow")?)
            .ok_or("truncated archive header")?;
        if &header[58..] != b"`\n" {
            return Err("invalid archive header trailer".into());
        }
        let size = decimal(&header[48..58])?;
        let start = cursor + 60;
        let end = start.checked_add(size).ok_or("archive member size overflow")?;
        let mut payload = bytes.get(start..end).ok_or("archive member exceeds input")?;
        let raw =
            std::str::from_utf8(&header[..16]).map_err(|_| "archive name is not UTF-8")?.trim_end();
        let name = if raw == "//" {
            long_names = Some(payload);
            None
        } else if raw == "/" || raw == "/SYM64/" {
            None
        } else if let Some(count) = raw.strip_prefix("#1/") {
            let length = decimal(count.as_bytes())?;
            charge_archive(&mut work, length, max_work)?;
            let name = payload.get(..length).ok_or("BSD archive name exceeds member")?;
            payload = &payload[length..];
            Some(
                std::str::from_utf8(name)
                    .map_err(|_| "archive name is not UTF-8")?
                    .trim_end_matches('\0'),
            )
        } else if let Some(offset) = raw.strip_prefix('/') {
            let offset = decimal(offset.as_bytes())?;
            let table = long_names.ok_or("archive long name table is missing")?;
            let suffix = table.get(offset..).ok_or("archive long name offset exceeds table")?;
            let remaining = max_work - work;
            let search = &suffix[..suffix.len().min(remaining.saturating_add(1))];
            let length = match search.iter().position(|b| *b == b'\n' || *b == 0) {
                Some(length) => length,
                None if suffix.len() > remaining => {
                    return Err("archive parsing work budget exceeded".into())
                },
                None => return Err("unterminated archive long name".into()),
            };
            charge_archive(&mut work, length + 1, max_work)?;
            Some(
                std::str::from_utf8(&suffix[..length])
                    .map_err(|_| "archive name is not UTF-8")?
                    .trim_end_matches('/'),
            )
        } else {
            Some(raw.trim_end_matches('/'))
        };
        if let Some(name) = name {
            if !name.starts_with("__.SYMDEF") {
                names = names
                    .checked_add(name.len())
                    .filter(|n| *n <= max_names)
                    .ok_or("archive name byte budget exceeded")?;
                charge_archive(
                    &mut work,
                    name.len().saturating_mul(2).saturating_add(1),
                    max_work,
                )?;
                if name.is_empty() || name.contains('\0') {
                    return Err("invalid archive member name".into());
                }
                if members.len() >= 4096 {
                    return Err("archive member limit exceeded".into());
                }
                members.push(ArchiveMember { name: name.to_owned(), bytes: payload });
            }
        }
        cursor = end;
        if size % 2 != 0 {
            if bytes.get(cursor) != Some(&b'\n') {
                return Err("missing archive odd-member padding".into());
            }
            cursor += 1;
        }
    }
    Ok((members, names, work))
}

fn decimal(bytes: &[u8]) -> Result<usize, String> {
    let value = std::str::from_utf8(bytes).map_err(|_| "invalid archive decimal field")?.trim();
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err("invalid archive decimal field".into());
    }
    value.parse().map_err(|_| "archive decimal field overflow".into())
}

pub struct ArchiveObject {
    pub name: String,
    pub object: ObjectFile,
}

/// Shared across ordinary objects and all archive members. A rejected input does
/// not alter the budget or the Linker input list. Limits reach the reader before
/// payload/name/table allocation, including unselected archive members.
#[derive(Clone, Copy)]
pub(crate) struct ReadBudget {
    remaining: ObjectReadLimits,
}

impl ReadBudget {
    pub(crate) fn new(limits: ObjectReadLimits) -> Self {
        Self { remaining: limits }
    }

    fn subtract(
        slot: &mut usize,
        amount: usize,
        resource: ObjectResource,
    ) -> Result<(), ObjectError> {
        *slot = slot.checked_sub(amount).ok_or_else(|| ObjectError::limit("read", resource))?;
        Ok(())
    }

    fn input(&mut self, bytes: usize) -> Result<(), ObjectError> {
        Self::subtract(&mut self.remaining.max_input_bytes, bytes, ObjectResource::InputBytes)
    }

    fn consume(&mut self, u: ObjectReadUsage) -> Result<(), ObjectError> {
        for (slot, amount, resource) in [
            (&mut self.remaining.max_sections, u.sections, ObjectResource::Sections),
            (
                &mut self.remaining.max_program_headers,
                u.program_headers,
                ObjectResource::ProgramHeaders,
            ),
            (&mut self.remaining.max_symbols, u.symbols, ObjectResource::Symbols),
            (&mut self.remaining.max_relocations, u.relocations, ObjectResource::Relocations),
            (&mut self.remaining.max_payload_bytes, u.payload_bytes, ObjectResource::PayloadBytes),
            (&mut self.remaining.max_string_bytes, u.string_bytes, ObjectResource::StringBytes),
            (&mut self.remaining.max_work, u.work, ObjectResource::Work),
        ] {
            Self::subtract(slot, amount, resource)?;
        }
        Ok(())
    }

    fn member(&mut self, bytes: &[u8]) -> Result<ObjectFile, ObjectError> {
        let mut limits = self.remaining;
        // Physical archive bytes were already charged at the outer boundary.
        limits.max_input_bytes = bytes.len();
        let (object, usage) = ObjectFile::read_with_usage(bytes, limits)?;
        self.consume(usage)?;
        Ok(object)
    }

    pub(crate) fn object(&mut self, bytes: &[u8]) -> Result<ObjectFile, crate::LinkError> {
        let mut next = *self;
        next.input(bytes.len()).map_err(input_error)?;
        let object = next.member(bytes).map_err(input_error)?;
        *self = next;
        Ok(object)
    }

    pub(crate) fn archive(&mut self, bytes: &[u8]) -> Result<Vec<ArchiveObject>, crate::LinkError> {
        let mut next = *self;
        next.input(bytes.len()).map_err(input_error)?;
        let (members, names, work) = parse_archive(
            bytes,
            next.remaining.max_string_bytes.min(1024 * 1024),
            next.remaining.max_work,
        )
        .map_err(|message| match message.as_str() {
            "archive name byte budget exceeded" => {
                input_error(ObjectError::limit("read", ObjectResource::StringBytes))
            },
            "archive parsing work budget exceeded" => {
                input_error(ObjectError::limit("read", ObjectResource::Work))
            },
            _ => crate::LinkError::new(crate::LinkErrorKind::Input, message),
        })?;
        Self::subtract(&mut next.remaining.max_string_bytes, names, ObjectResource::StringBytes)
            .map_err(input_error)?;
        Self::subtract(&mut next.remaining.max_work, work, ObjectResource::Work)
            .map_err(input_error)?;
        let mut objects = Vec::new();
        for member in members {
            let object = next.member(member.bytes).map_err(|cause| {
                let mut error = input_error(cause);
                error.message = format!("archive member {:?}: {}", member.name, error.message);
                error.contexts.push(whale_diagnostic::Context::Object {
                    name: "archive".into(),
                    member: Some(member.name.clone()),
                });
                error
            })?;
            objects.push(ArchiveObject { name: member.name, object });
        }
        *self = next;
        Ok(objects)
    }
}

fn input_error(cause: ObjectError) -> crate::LinkError {
    let mut error = crate::LinkError::new(crate::LinkErrorKind::Input, cause.to_string());
    error.contexts = cause.contexts.clone();
    error.input_cause = Some(Box::new(cause));
    error
}

pub fn read_objects(bytes: &[u8]) -> Result<Vec<ArchiveObject>, crate::LinkError> {
    read_objects_with_limits(bytes, ObjectReadLimits::default())
}

/// Apply one cumulative reader budget to the entire archive, even unused members.
pub fn read_objects_with_limits(
    bytes: &[u8],
    limits: ObjectReadLimits,
) -> Result<Vec<ArchiveObject>, crate::LinkError> {
    ReadBudget::new(limits).archive(bytes)
}
