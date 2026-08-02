//! Per-format compression level configuration for [`crate::ArchiveBuilder`].

use crate::error::{ArchiveError, Result};
use crate::format::ArchiveFormat;

/// Zip's write-side compression method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZipCompression {
    /// No compression -- data is stored verbatim.
    Stored,
    /// Deflate at the given level, 0 (no compression) - 9 (best). Default: 6.
    Deflated(u32),
}

/// Requested compression effort for [`crate::ArchiveBuilder::compression_level`].
///
/// Each variant's range mirrors the backend crate it maps to, and each
/// (other than [`CompressionLevel::Default`]) only applies to the archive
/// format(s) named in its docs. Building with a variant that doesn't apply
/// to the target format (e.g. [`CompressionLevel::Xz`] while building
/// [`ArchiveFormat::Zip`]), or with a value outside its documented range,
/// returns [`ArchiveError::InvalidCompressionLevel`].
///
/// # Examples
///
/// ```
/// use archive::{ArchiveBuilder, ArchiveEntry, ArchiveFormat, CompressionLevel};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let entries = vec![ArchiveEntry::file("hello.txt", b"Hello, World!".to_vec())];
///
/// let bytes = ArchiveBuilder::new()
///     .compression_level(CompressionLevel::Xz(9))
///     .build(&entries, ArchiveFormat::TarXz)?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionLevel {
    /// Use each backend's own default (flate2/bzip2 level 6, zstd level 3,
    /// xz preset 6, lz4's fast mode). Applies to every format.
    #[default]
    Default,

    /// Zip's compression method and, for `Deflated`, its level.
    /// Applies to [`ArchiveFormat::Zip`] only.
    Zip(ZipCompression),

    /// flate2 deflate level, 0 (no compression) - 9 (best). Default: 6.
    /// Applies to [`ArchiveFormat::Gz`] and [`ArchiveFormat::TarGz`].
    Gzip(u32),

    /// bzip2 level, 1 (fastest) - 9 (best). Default: 6.
    /// Applies to [`ArchiveFormat::Bz2`] and [`ArchiveFormat::TarBz2`].
    Bzip2(u32),

    /// zstd level, -7 (fastest) - 22 (best); 0 requests zstd's own default
    /// level (currently 3).
    /// Applies to [`ArchiveFormat::Zst`] and [`ArchiveFormat::TarZst`].
    Zstd(i32),

    /// xz/LZMA2 preset, 0 (fastest) - 9 (best). Default: 6.
    /// Applies to [`ArchiveFormat::Xz`] and [`ArchiveFormat::TarXz`].
    Xz(u32),

    /// lz4 level, 0 (fast mode) - 16 (best, high-compression mode).
    /// Default: 0.
    /// Applies to [`ArchiveFormat::Lz4`] and [`ArchiveFormat::TarLz4`].
    Lz4(u32),
}

impl CompressionLevel {
    fn mismatch(self, format: ArchiveFormat) -> ArchiveError {
        ArchiveError::InvalidCompressionLevel(format!(
            "{self:?} does not apply to {} archives",
            format.name()
        ))
    }

    fn out_of_range(self, expected: &str) -> ArchiveError {
        ArchiveError::InvalidCompressionLevel(format!(
            "{self:?} is out of range (expected {expected})"
        ))
    }

    pub(crate) fn to_flate2(self, format: ArchiveFormat) -> Result<flate2::Compression> {
        match self {
            Self::Default => Ok(flate2::Compression::default()),
            Self::Gzip(level) if level <= 9 => Ok(flate2::Compression::new(level)),
            Self::Gzip(_) => Err(self.out_of_range("0..=9")),
            _ => Err(self.mismatch(format)),
        }
    }

    pub(crate) fn to_bzip2(self, format: ArchiveFormat) -> Result<bzip2::Compression> {
        match self {
            Self::Default => Ok(bzip2::Compression::default()),
            Self::Bzip2(level) if (1..=9).contains(&level) => Ok(bzip2::Compression::new(level)),
            Self::Bzip2(_) => Err(self.out_of_range("1..=9")),
            _ => Err(self.mismatch(format)),
        }
    }

    pub(crate) fn to_zstd(self, format: ArchiveFormat) -> Result<i32> {
        match self {
            // 0 asks zstd for its own default (currently 3).
            Self::Default => Ok(0),
            Self::Zstd(level) if (-7..=22).contains(&level) => Ok(level),
            Self::Zstd(_) => Err(self.out_of_range("-7..=22")),
            _ => Err(self.mismatch(format)),
        }
    }

    pub(crate) fn to_xz_preset(self, format: ArchiveFormat) -> Result<u32> {
        match self {
            Self::Default => Ok(6),
            Self::Xz(preset) if preset <= 9 => Ok(preset),
            Self::Xz(_) => Err(self.out_of_range("0..=9")),
            _ => Err(self.mismatch(format)),
        }
    }

    pub(crate) fn to_lz4(self, format: ArchiveFormat) -> Result<u32> {
        match self {
            Self::Default => Ok(0),
            Self::Lz4(level) if level <= 16 => Ok(level),
            Self::Lz4(_) => Err(self.out_of_range("0..=16")),
            _ => Err(self.mismatch(format)),
        }
    }

    /// Builds the base [`zip::write::FileOptions`] carrying the compression
    /// method/level, shared by every entry in the archive. Per-entry mode
    /// and mtime are layered on top by the caller.
    pub(crate) fn to_zip_options(self) -> Result<zip::write::FileOptions<'static, ()>> {
        let options = zip::write::FileOptions::default();
        match self {
            Self::Default => Ok(options),
            Self::Zip(ZipCompression::Stored) => {
                Ok(options.compression_method(zip::CompressionMethod::Stored))
            }
            Self::Zip(ZipCompression::Deflated(level)) if level <= 9 => Ok(options
                .compression_method(zip::CompressionMethod::Deflated)
                .compression_level(Some(level as i64))),
            Self::Zip(ZipCompression::Deflated(_)) => Err(self.out_of_range("0..=9")),
            _ => Err(self.mismatch(ArchiveFormat::Zip)),
        }
    }
}
