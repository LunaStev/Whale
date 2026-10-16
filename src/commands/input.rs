use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

pub(super) const ASM_INPUT_BYTES: u64 = 8 * 1024 * 1024;

pub(super) const OBJECT_INPUT_BYTES: u64 = 256 * 1024 * 1024;

pub(super) fn byte_limit(value: &str) -> Result<u64, &'static str> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err("--max-input requires a decimal byte count");
    }
    value
        .parse::<u64>()
        .ok()
        .filter(|v| *v < usize::MAX as u64)
        .ok_or("--max-input byte count exceeds host capacity")
}

pub(super) fn read(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    read_from(File::open(path)?, limit)
}

fn read_from(reader: impl Read, limit: u64) -> io::Result<Vec<u8>> {
    let cap = limit
        .checked_add(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "input limit overflow"))?;
    let mut bytes = Vec::new();
    reader.take(cap).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("input byte limit exceeded (limit {limit} bytes)"),
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::Cell;

    struct Growing<'a>(&'a Cell<usize>);

    impl Read for Growing<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            buffer.fill(b'x');
            self.0.set(self.0.get() + buffer.len());
            Ok(buffer.len())
        }
    }

    #[test]
    fn growing_stream_is_consumed_only_to_limit_plus_one() {
        let consumed = Cell::new(0);
        let error = read_from(Growing(&consumed), 32).unwrap_err();
        assert!(error.to_string().contains("input byte limit"));
        assert_eq!(consumed.get(), 33);
        assert_eq!(read_from(&b"abc"[..], 3).unwrap(), b"abc");
        assert_eq!(read_from(&b""[..], 0).unwrap(), b"");
    }
}
