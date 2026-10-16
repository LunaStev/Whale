//! Bounded ordinary Unix ar reader. Thin archives and dynamic inputs are rejected.
use object::ObjectFile;
#[derive(Debug)]
pub struct ArchiveMember<'a> {
    pub name: String,
    pub bytes: &'a [u8],
}
pub fn read_archive(bytes: &[u8]) -> Result<Vec<ArchiveMember<'_>>, String> {
    if !bytes.starts_with(b"!<arch>\n") {
        return Err("expected ordinary ar archive (thin archives are unsupported)".into());
    }
    if bytes.len() > 256 * 1024 * 1024 {
        return Err("archive input byte limit exceeded".into());
    }
    let mut cursor = 8usize;
    let mut members = Vec::new();
    let mut names = 0usize;
    let mut long_names = None;
    while cursor < bytes.len() {
        let header = bytes
            .get(cursor..cursor.checked_add(60).ok_or("archive header overflow")?)
            .ok_or("truncated archive header")?;
        if &header[58..] != b"`\n" {
            return Err("invalid archive header trailer".into());
        }
        let size = decimal(&header[48..58])?;
        let start = cursor + 60;
        let end = start
            .checked_add(size)
            .ok_or("archive member size overflow")?;
        let mut payload = bytes
            .get(start..end)
            .ok_or("archive member exceeds input")?;
        let raw = std::str::from_utf8(&header[..16])
            .map_err(|_| "archive name is not UTF-8")?
            .trim_end();
        let name = if raw == "//" {
            long_names = Some(payload);
            None
        } else if raw == "/" || raw == "/SYM64/" {
            None
        } else if let Some(count) = raw.strip_prefix("#1/") {
            let length = decimal(count.as_bytes())?;
            let name = payload
                .get(..length)
                .ok_or("BSD archive name exceeds member")?;
            payload = &payload[length..];
            Some(
                std::str::from_utf8(name)
                    .map_err(|_| "archive name is not UTF-8")?
                    .trim_end_matches('\0')
                    .to_owned(),
            )
        } else if let Some(offset) = raw.strip_prefix('/') {
            let offset = decimal(offset.as_bytes())?;
            let table = long_names.ok_or("archive long name table is missing")?;
            let suffix = table
                .get(offset..)
                .ok_or("archive long name offset exceeds table")?;
            let length = suffix
                .iter()
                .position(|b| *b == b'\n' || *b == 0)
                .ok_or("unterminated archive long name")?;
            Some(
                std::str::from_utf8(&suffix[..length])
                    .map_err(|_| "archive name is not UTF-8")?
                    .trim_end_matches('/')
                    .to_owned(),
            )
        } else {
            Some(raw.trim_end_matches('/').to_owned())
        };
        if let Some(name) = name {
            if !name.starts_with("__.SYMDEF") {
                if name.is_empty() || name.contains('\0') {
                    return Err("invalid archive member name".into());
                }
                names = names
                    .checked_add(name.len())
                    .filter(|n| *n <= 1024 * 1024)
                    .ok_or("archive name byte budget exceeded")?;
                if members.len() >= 4096 {
                    return Err("archive member limit exceeded".into());
                }
                members.push(ArchiveMember {
                    name,
                    bytes: payload,
                });
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
    Ok(members)
}
fn decimal(bytes: &[u8]) -> Result<usize, String> {
    let value = std::str::from_utf8(bytes)
        .map_err(|_| "invalid archive decimal field")?
        .trim();
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err("invalid archive decimal field".into());
    }
    value
        .parse()
        .map_err(|_| "archive decimal field overflow".into())
}
pub struct ArchiveObject {
    pub name: String,
    pub object: ObjectFile,
}
pub fn read_objects(bytes: &[u8]) -> Result<Vec<ArchiveObject>, crate::LinkError> {
    use crate::{LinkError, LinkErrorKind};
    read_archive(bytes)
        .map_err(|e| LinkError::new(LinkErrorKind::Input, e))?
        .into_iter()
        .map(|m| {
            ObjectFile::read(m.bytes)
                .map(|object| ArchiveObject {
                    name: m.name.clone(),
                    object,
                })
                .map_err(|e| {
                    let mut error = LinkError::new(
                        LinkErrorKind::Input,
                        format!("archive member {:?}: {e}", m.name),
                    );
                    error.contexts = e.contexts.clone();
                    error.contexts.push(whale_diagnostic::Context::Object {
                        name: "archive".into(),
                        member: Some(m.name),
                    });
                    error.input_cause = Some(Box::new(e));
                    error
                })
        })
        .collect()
}
