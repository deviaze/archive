//! Archive creation implementations.
//!
//! This module provides the inverse of [`crate::extractor`]: turning a list
//! of [`ArchiveEntry`] values into the raw bytes of an archive. The main
//! entry point is [`ArchiveBuilder`].

use crate::compression::CompressionLevel;
use crate::error::{ArchiveError, Result};
use crate::extractor::ArchiveEntry;
use crate::format::ArchiveFormat;
use crate::path_safety::validate_path;
use std::io::{Cursor, Write};
use std::time::SystemTime;

/// Builds archives in-memory from a list of [`ArchiveEntry`] values.
///
/// This is the inverse of [`crate::ArchiveExtractor`]: instead of turning
/// archive bytes into entries, it turns entries into archive bytes. Entry
/// paths (and symlink targets) are validated the same way extraction
/// validates them, so a caller assembling entries from less-trusted
/// filenames can't accidentally produce an archive containing `..` or
/// absolute paths.
///
/// Supports every format [`crate::ArchiveExtractor`] can read, except
/// `.7z` symlinks (this crate's `.7z` reader doesn't recognize symlinks
/// either, so there's nothing for a written one to round-trip against) and
/// `.ar`/`.deb` directories or symlinks (the ar format has no concept of
/// either — it's a flat list of named byte blobs).
///
/// # Examples
///
/// ```
/// use archive::{ArchiveBuilder, ArchiveEntry, ArchiveFormat};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let entries = vec![
///     ArchiveEntry::file("hello.txt", b"Hello, World!".to_vec()),
///     ArchiveEntry::directory("empty-dir"),
/// ];
///
/// let builder = ArchiveBuilder::new();
/// let zip_bytes = builder.build(&entries, ArchiveFormat::Zip)?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Default)]
pub struct ArchiveBuilder {
    allow_unsafe_path_traversals: bool,
    compression_level: CompressionLevel,
}

impl ArchiveBuilder {
    /// Creates a new archive builder.
    ///
    /// # Examples
    ///
    /// ```
    /// use archive::ArchiveBuilder;
    ///
    /// let builder = ArchiveBuilder::new();
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Controls whether entry paths (and symlink targets) are allowed to
    /// contain `..` components or be absolute.
    ///
    /// By default (`false`), building rejects any such entry with
    /// [`ArchiveError::UnsafePath`] to protect against producing an archive
    /// that would path-traverse on extraction. Only set this to `true` if
    /// the caller has a specific reason to write such paths.
    ///
    /// This method uses the builder pattern, allowing you to chain configuration calls.
    ///
    /// # Examples
    ///
    /// ```
    /// use archive::ArchiveBuilder;
    ///
    /// let builder = ArchiveBuilder::new().allow_unsafe_path_traversals(true);
    /// ```
    pub fn allow_unsafe_path_traversals(mut self, allow: bool) -> Self {
        self.allow_unsafe_path_traversals = allow;
        self
    }

    /// Sets the compression effort used when building the archive.
    ///
    /// [`CompressionLevel`] is format-specific: each variant (other than
    /// [`CompressionLevel::Default`]) only applies to the archive format(s)
    /// documented on it. Building with a variant that doesn't apply to the
    /// target format, or whose value is out of range, fails with
    /// [`ArchiveError::InvalidCompressionLevel`].
    ///
    /// This method uses the builder pattern, allowing you to chain configuration calls.
    ///
    /// # Examples
    ///
    /// ```
    /// use archive::{ArchiveBuilder, CompressionLevel};
    ///
    /// let builder = ArchiveBuilder::new().compression_level(CompressionLevel::Xz(9));
    /// ```
    pub fn compression_level(mut self, level: CompressionLevel) -> Self {
        self.compression_level = level;
        self
    }

    /// Builds an archive from a list of entries.
    ///
    /// # Arguments
    ///
    /// * `entries` - The files, directories, and symlinks to include
    /// * `format` - The archive format to produce (see [`ArchiveFormat`])
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - Any entry's path (or, for symlinks, its target) is unsafe
    ///   ([`ArchiveError::UnsafePath`])
    /// - An entry kind isn't representable in `format` — e.g. a directory
    ///   or symlink for `.ar`/`.deb`, or a symlink for `.7z`
    ///   ([`ArchiveError::UnsupportedFormat`])
    /// - An I/O error occurs while writing ([`ArchiveError::Io`])
    ///
    /// # Examples
    ///
    /// ```
    /// use archive::{ArchiveBuilder, ArchiveEntry, ArchiveFormat};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let entries = vec![ArchiveEntry::file("hello.txt", b"Hello, World!".to_vec())];
    ///
    /// let bytes = ArchiveBuilder::new().build(&entries, ArchiveFormat::TarGz)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn build(&self, entries: &[ArchiveEntry], format: ArchiveFormat) -> Result<Vec<u8>> {
        match format {
            ArchiveFormat::Zip => self.build_zip(entries),
            ArchiveFormat::Tar => self.build_tar(entries),
            ArchiveFormat::TarGz => self.build_tar_gz(entries),
            ArchiveFormat::TarBz2 => self.build_tar_bz2(entries),
            ArchiveFormat::TarXz => self.build_tar_xz(entries),
            ArchiveFormat::TarZst => self.build_tar_zst(entries),
            ArchiveFormat::TarLz4 => self.build_tar_lz4(entries),
            ArchiveFormat::Ar => self.build_ar(entries),
            ArchiveFormat::Deb => self.build_ar(entries),
            ArchiveFormat::SevenZ => self.build_7z(entries),
            ArchiveFormat::Gz => self.build_single_gz(entries),
            ArchiveFormat::Bz2 => self.build_single_bz2(entries),
            ArchiveFormat::Xz => self.build_single_xz(entries),
            ArchiveFormat::Lz4 => self.build_single_lz4(entries),
            ArchiveFormat::Zst => self.build_single_zst(entries),
        }
    }

    /// Compresses a single file for one of the single-file formats
    /// (`.gz`, `.bz2`, `.xz`, `.lz4`, `.zst`).
    ///
    /// These formats have no container, so they can only ever hold one
    /// file — going through [`Self::build`] means wrapping a single entry
    /// in a `vec![...]` just to satisfy its general signature. This skips
    /// that.
    ///
    /// # Errors
    ///
    /// Returns [`ArchiveError::UnsupportedFormat`] if `format` isn't one
    /// of the single-file formats (use [`Self::build`] for containers like
    /// `.zip`/`.tar.gz`, which can hold more than one entry).
    ///
    /// # Examples
    ///
    /// ```
    /// use archive::{ArchiveBuilder, ArchiveFormat};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let bytes = ArchiveBuilder::new().build_single_file(
    ///     "hello.txt",
    ///     b"Hello, World!".to_vec(),
    ///     ArchiveFormat::Gz,
    /// )?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn build_single_file(
        &self,
        path: impl AsRef<std::path::Path>,
        data: impl Into<Vec<u8>>,
        format: ArchiveFormat,
    ) -> Result<Vec<u8>> {
        match format {
            ArchiveFormat::Gz
            | ArchiveFormat::Bz2
            | ArchiveFormat::Xz
            | ArchiveFormat::Lz4
            | ArchiveFormat::Zst => self.build(&[ArchiveEntry::file(path, data)], format),
            _ => Err(ArchiveError::UnsupportedFormat(format!(
                "{} is not a single-file format; use build() instead",
                format.name()
            ))),
        }
    }

    /// Layers per-entry `mode` and `mtime`, if present, onto a base
    /// [`zip::write::FileOptions`] that already carries the builder's
    /// compression settings. An `mtime` that predates 1980 or postdates
    /// 2107 (outside what zip's MS-DOS timestamp can represent) is silently
    /// omitted rather than rejected — it's best-effort metadata, not
    /// something worth failing the whole build over.
    fn zip_file_options(
        base: zip::write::FileOptions<'static, ()>,
        mode: Option<u32>,
        mtime: Option<SystemTime>,
    ) -> zip::write::FileOptions<'static, ()> {
        let mut options = base;
        if let Some(mode) = mode {
            options = options.unix_permissions(mode);
        }
        if let Some(dt) = mtime.and_then(crate::extractor::system_time_to_zip_datetime) {
            options = options.last_modified_time(dt);
        }
        options
    }

    fn build_zip(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let base_options = self.compression_level.to_zip_options()?;

        for entry in entries {
            match entry {
                ArchiveEntry::File { path, data, mode, mtime } => {
                    validate_path(path, self.allow_unsafe_path_traversals)?;
                    let options = Self::zip_file_options(base_options, *mode, *mtime);
                    writer.start_file(path, options)?;
                    writer.write_all(data)?;
                }
                ArchiveEntry::Directory { path, mode, mtime } => {
                    validate_path(path, self.allow_unsafe_path_traversals)?;
                    let options = Self::zip_file_options(base_options, *mode, *mtime);
                    writer.add_directory(path.clone(), options)?;
                }
                ArchiveEntry::Symlink { path, target, mode, mtime } => {
                    validate_path(path, self.allow_unsafe_path_traversals)?;
                    validate_path(target, self.allow_unsafe_path_traversals)?;
                    let options = Self::zip_file_options(base_options, *mode, *mtime);
                    writer.add_symlink(path, target, options)?;
                }
            }
        }

        Ok(writer.finish()?.into_inner())
    }

    /// Converts an `mtime` into the Unix-seconds form `tar::Header::set_mtime`
    /// expects. A time before 1970 can't be represented in tar's mtime
    /// field, so it's silently omitted (left at the header default) rather
    /// than rejected — like zip's mtime, this is best-effort metadata.
    fn tar_mtime_secs(mtime: Option<SystemTime>) -> Option<u64> {
        mtime.and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
    }

    /// Appends `entries` onto a tar builder writing to any `Write` sink.
    /// Shared by every tar-family format (plain and each compression
    /// wrapper) since they only differ in what wraps the underlying writer.
    fn write_tar_entries<W: Write>(
        builder: &mut tar::Builder<W>,
        entries: &[ArchiveEntry],
        allow_unsafe_path_traversals: bool,
    ) -> Result<()> {
        for entry in entries {
            match entry {
                ArchiveEntry::File { path, data, mode, mtime } => {
                    validate_path(path, allow_unsafe_path_traversals)?;
                    let mut header = tar::Header::new_gnu();
                    header.set_size(data.len() as u64);
                    header.set_mode(mode.unwrap_or(0o644));
                    if let Some(secs) = Self::tar_mtime_secs(*mtime) {
                        header.set_mtime(secs);
                    }
                    header.set_cksum();
                    builder.append_data(&mut header, path, data.as_slice())?;
                }
                ArchiveEntry::Directory { path, mode, mtime } => {
                    validate_path(path, allow_unsafe_path_traversals)?;
                    let mut header = tar::Header::new_gnu();
                    header.set_size(0);
                    header.set_mode(mode.unwrap_or(0o755));
                    if let Some(secs) = Self::tar_mtime_secs(*mtime) {
                        header.set_mtime(secs);
                    }
                    header.set_entry_type(tar::EntryType::Directory);
                    header.set_cksum();
                    builder.append_data(&mut header, path, std::io::empty())?;
                }
                ArchiveEntry::Symlink { path, target, mode, mtime } => {
                    validate_path(path, allow_unsafe_path_traversals)?;
                    validate_path(target, allow_unsafe_path_traversals)?;
                    let mut header = tar::Header::new_gnu();
                    header.set_size(0);
                    if let Some(mode) = mode {
                        header.set_mode(*mode);
                    }
                    if let Some(secs) = Self::tar_mtime_secs(*mtime) {
                        header.set_mtime(secs);
                    }
                    header.set_entry_type(tar::EntryType::Symlink);
                    header.set_cksum();
                    builder.append_link(&mut header, path, target)?;
                }
            }
        }
        Ok(())
    }

    fn build_tar(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let mut builder = tar::Builder::new(Vec::new());
        Self::write_tar_entries(&mut builder, entries, self.allow_unsafe_path_traversals)?;
        Ok(builder.into_inner()?)
    }

    fn build_tar_gz(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let level = self.compression_level.to_flate2(ArchiveFormat::TarGz)?;
        let encoder = flate2::write::GzEncoder::new(Vec::new(), level);
        let mut builder = tar::Builder::new(encoder);
        Self::write_tar_entries(&mut builder, entries, self.allow_unsafe_path_traversals)?;
        Ok(builder.into_inner()?.finish()?)
    }

    fn build_tar_bz2(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let level = self.compression_level.to_bzip2(ArchiveFormat::TarBz2)?;
        let encoder = bzip2::write::BzEncoder::new(Vec::new(), level);
        let mut builder = tar::Builder::new(encoder);
        Self::write_tar_entries(&mut builder, entries, self.allow_unsafe_path_traversals)?;
        Ok(builder.into_inner()?.finish()?)
    }

    fn build_tar_zst(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let level = self.compression_level.to_zstd(ArchiveFormat::TarZst)?;
        let encoder = zstd::stream::write::Encoder::new(Vec::new(), level)?;
        let mut builder = tar::Builder::new(encoder);
        Self::write_tar_entries(&mut builder, entries, self.allow_unsafe_path_traversals)?;
        Ok(builder.into_inner()?.finish()?)
    }

    fn build_tar_lz4(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let level = self.compression_level.to_lz4(ArchiveFormat::TarLz4)?;
        let encoder = lz4::EncoderBuilder::new().level(level).build(Vec::new())?;
        let mut builder = tar::Builder::new(encoder);
        Self::write_tar_entries(&mut builder, entries, self.allow_unsafe_path_traversals)?;
        let (buf, result) = builder.into_inner()?.finish();
        result?;
        Ok(buf)
    }

    /// Encoding goes through `liblzma` rather than `lzma-rs` (which the
    /// extractor still decodes with) because `lzma-rs`' LZMA2 encoder only
    /// ever emits `uncompressed reset dict` chunks — it produces a valid xz
    /// stream that is *larger* than its input, so a `.tar.xz` built with it
    /// came out bigger than the plain `.tar`. `lzma-rs`' decoder is a real
    /// implementation, so it's kept for reading.
    fn build_tar_xz(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let preset = self.compression_level.to_xz_preset(ArchiveFormat::TarXz)?;
        let encoder = liblzma::write::XzEncoder::new(Vec::new(), preset);
        let mut builder = tar::Builder::new(encoder);
        Self::write_tar_entries(&mut builder, entries, self.allow_unsafe_path_traversals)?;
        Ok(builder.into_inner()?.finish()?)
    }

    fn build_ar(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let mut builder = ar::Builder::new(Vec::new());

        for entry in entries {
            match entry {
                ArchiveEntry::File { path, data, mode, mtime } => {
                    validate_path(path, self.allow_unsafe_path_traversals)?;
                    let mut header = ar::Header::new(path.clone().into_bytes(), data.len() as u64);
                    // ar::Header::new defaults mode to 0, unlike the tar/zip paths which fall
                    // back to a sane default when the entry doesn't carry one; without this,
                    // an entry built without with_mode() round-trips as an unreadable 0-permission file.
                    header.set_mode(mode.unwrap_or(0o644));
                    if let Some(secs) = Self::tar_mtime_secs(*mtime) {
                        header.set_mtime(secs);
                    }
                    builder.append(&header, data.as_slice())?;
                }
                ArchiveEntry::Directory { .. } | ArchiveEntry::Symlink { .. } => {
                    return Err(ArchiveError::UnsupportedFormat(
                        "ar/deb archives can only contain files (no directories or symlinks)"
                            .to_string(),
                    ));
                }
            }
        }

        Ok(builder.into_inner()?)
    }

    /// Sets `mode` and `mtime` on a 7z entry being built, mirroring how
    /// [`crate::extractor::ArchiveExtractor`] reads them back: `mode` is
    /// packed into the upper 16 bits of `windows_attributes` behind the
    /// `FILE_ATTRIBUTE_UNIX_EXTENSION` flag, and `mtime` is converted to an
    /// NT file time. A time outside what `FileTime` can represent is
    /// silently omitted, same as the other formats.
    fn apply_7z_metadata(
        sz_entry: &mut sevenz_rust::SevenZArchiveEntry,
        mode: Option<u32>,
        mtime: Option<SystemTime>,
    ) {
        if let Some(mode) = mode {
            sz_entry.has_windows_attributes = true;
            sz_entry.windows_attributes = (mode << 16) | crate::extractor::SEVENZ_UNIX_EXTENSION_FLAG;
        }
        if let Some(mtime) = mtime
            && let Ok(file_time) = sevenz_rust::nt_time::FileTime::try_from(mtime)
        {
            sz_entry.has_last_modified_date = true;
            sz_entry.last_modified_date = file_time;
        }
    }

    fn build_7z(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let mut writer = sevenz_rust::SevenZWriter::new(Cursor::new(Vec::new()))
            .map_err(|e| ArchiveError::InvalidArchive(format!("7z error: {}", e)))?;

        for entry in entries {
            match entry {
                ArchiveEntry::File { path, data, mode, mtime } => {
                    validate_path(path, self.allow_unsafe_path_traversals)?;
                    let mut sz_entry = sevenz_rust::SevenZArchiveEntry::new();
                    sz_entry.name = path.clone();
                    sz_entry.has_stream = true;
                    Self::apply_7z_metadata(&mut sz_entry, *mode, *mtime);
                    writer
                        .push_archive_entry(sz_entry, Some(Cursor::new(data.clone())))
                        .map_err(|e| ArchiveError::InvalidArchive(format!("7z error: {}", e)))?;
                }
                ArchiveEntry::Directory { path, mode, mtime } => {
                    validate_path(path, self.allow_unsafe_path_traversals)?;
                    let mut sz_entry = sevenz_rust::SevenZArchiveEntry::new();
                    sz_entry.name = path.clone();
                    sz_entry.is_directory = true;
                    Self::apply_7z_metadata(&mut sz_entry, *mode, *mtime);
                    writer
                        .push_archive_entry::<Cursor<Vec<u8>>>(sz_entry, None)
                        .map_err(|e| ArchiveError::InvalidArchive(format!("7z error: {}", e)))?;
                }
                ArchiveEntry::Symlink { .. } => {
                    return Err(ArchiveError::UnsupportedFormat(
                        "7z symlinks aren't supported for creation (this crate's 7z reader \
                         doesn't recognize them either)"
                            .to_string(),
                    ));
                }
            }
        }

        let cursor = writer
            .finish()
            .map_err(|e| ArchiveError::InvalidArchive(format!("7z error: {}", e)))?;
        Ok(cursor.into_inner())
    }

    /// Builds a single-file compressed archive (`.gz`/`.bz2`/`.xz`/`.lz4`/`.zst`).
    ///
    /// These formats have no container: they can only hold exactly one
    /// [`ArchiveEntry::File`].
    fn single_file_entry(
        entries: &[ArchiveEntry],
        allow_unsafe_path_traversals: bool,
    ) -> Result<(String, Vec<u8>, Option<SystemTime>)> {
        let [entry] = entries else {
            return Err(ArchiveError::UnsupportedFormat(format!(
                "single-file compressed archives must contain exactly one file entry, got {}",
                entries.len()
            )));
        };

        match entry {
            ArchiveEntry::File { path, data, mtime, .. } => {
                validate_path(path, allow_unsafe_path_traversals)?;
                Ok((path.clone(), data.clone(), *mtime))
            }
            ArchiveEntry::Directory { .. } | ArchiveEntry::Symlink { .. } => {
                Err(ArchiveError::UnsupportedFormat(
                    "single-file compressed archives can only contain a file entry".to_string(),
                ))
            }
        }
    }

    fn build_single_gz(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let (path, data, mtime) = Self::single_file_entry(entries, self.allow_unsafe_path_traversals)?;
        let mut header = flate2::GzBuilder::new().filename(path);
        // Gzip's MTIME field is a u32 of seconds since 1970; a time outside
        // that range (or before it) is silently omitted, same as elsewhere.
        if let Some(secs) = mtime
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .and_then(|d| u32::try_from(d.as_secs()).ok())
        {
            header = header.mtime(secs);
        }
        let level = self.compression_level.to_flate2(ArchiveFormat::Gz)?;
        let mut encoder = header.write(Vec::new(), level);
        encoder.write_all(&data)?;
        Ok(encoder.finish()?)
    }

    fn build_single_bz2(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let (_path, data, _mtime) = Self::single_file_entry(entries, self.allow_unsafe_path_traversals)?;
        let level = self.compression_level.to_bzip2(ArchiveFormat::Bz2)?;
        let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), level);
        encoder.write_all(&data)?;
        Ok(encoder.finish()?)
    }

    fn build_single_xz(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let (_path, data, _mtime) = Self::single_file_entry(entries, self.allow_unsafe_path_traversals)?;
        let preset = self.compression_level.to_xz_preset(ArchiveFormat::Xz)?;
        let mut encoder = liblzma::write::XzEncoder::new(Vec::new(), preset);
        encoder.write_all(&data)?;
        Ok(encoder.finish()?)
    }

    fn build_single_lz4(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let (_path, data, _mtime) = Self::single_file_entry(entries, self.allow_unsafe_path_traversals)?;
        let level = self.compression_level.to_lz4(ArchiveFormat::Lz4)?;
        let mut encoder = lz4::EncoderBuilder::new().level(level).build(Vec::new())?;
        encoder.write_all(&data)?;
        let (buf, result) = encoder.finish();
        result?;
        Ok(buf)
    }

    fn build_single_zst(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let (_path, data, _mtime) = Self::single_file_entry(entries, self.allow_unsafe_path_traversals)?;
        let level = self.compression_level.to_zstd(ArchiveFormat::Zst)?;
        let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), level)?;
        encoder.write_all(&data)?;
        Ok(encoder.finish()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let _builder = ArchiveBuilder::new();
    }
}
