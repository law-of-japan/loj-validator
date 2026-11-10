//! Input byte budget shared by the loj/v1alpha1 and loj/v1alpha2 loaders.
use std::{fs, io::Read, path::Path};

/// Bytes that may still be read: a total over all files and a per-file limit.
pub(crate) struct ReadBudget {
    pub(crate) remaining: usize,
    pub(crate) per_file: u64,
}

impl ReadBudget {
    pub(crate) fn new(total: usize, per_file: u64) -> Self {
        Self {
            remaining: total,
            per_file,
        }
    }

    fn limit(&self) -> u64 {
        self.per_file.min(self.remaining as u64)
    }

    // One bounded lookahead byte detects growth after metadata inspection.
    // It is never retained; the caller stops loading on overflow.
    pub(crate) fn read(&mut self, reader: impl Read) -> std::io::Result<(Vec<u8>, bool)> {
        let limit = self.limit();
        let mut bytes = Vec::new();
        let result = reader.take(limit + 1).read_to_end(&mut bytes);
        self.remaining = self.remaining.saturating_sub(bytes.len());
        result?;
        let exceeded = bytes.len() as u64 > limit;
        Ok((bytes, exceeded))
    }

    /// Read a regular file within the budget. `Ok(None)` means the file is
    /// larger than what remains; nothing of it is retained.
    pub(crate) fn read_file(&mut self, path: &Path) -> std::io::Result<Option<Vec<u8>>> {
        if !fs::metadata(path)?.is_file() {
            return Err(std::io::Error::other("Not a regular file"));
        }
        let f = fs::File::open(path)?;
        let meta = f.metadata()?;
        if !meta.is_file() {
            return Err(std::io::Error::other("Not a regular file"));
        }
        if meta.len() > self.limit() {
            return Ok(None);
        }
        let (bytes, exceeded) = self.read(f)?;
        Ok((!exceeded).then_some(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_read_is_bounded_even_when_length_is_unknown_or_changes() {
        for (remaining, per_file, expected_read) in [(7, 100, 8), (100, 7, 8)] {
            let mut budget = ReadBudget {
                remaining,
                per_file,
            };
            let (bytes, exceeded) = budget.read(std::io::repeat(b'x')).unwrap();
            assert!(exceeded);
            assert_eq!(bytes.len(), expected_read);
            assert_eq!(budget.remaining, remaining.saturating_sub(expected_read));
        }
        let mut budget = ReadBudget {
            remaining: 4,
            per_file: 4,
        };
        assert!(!budget.read(b"abcd".as_slice()).unwrap().1);
        assert_eq!(budget.remaining, 0);
    }

    #[test]
    fn partial_io_failure_still_consumes_budget() {
        let mut budget = ReadBudget {
            remaining: 8,
            per_file: 8,
        };
        struct Fails;
        impl Read for Fails {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("simulated failure"))
            }
        }
        assert!(budget.read(b"abcd".as_slice().chain(Fails)).is_err());
        assert_eq!(budget.remaining, 4);
    }
}
