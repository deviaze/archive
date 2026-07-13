//! Archive creation implementations.
//!
//! This module provides the inverse of [`crate::extractor`]: turning a list
//! of [`ArchiveEntry`] values into the raw bytes of an archive. The main
//! entry point is [`ArchiveBuilder`].

use crate::error::{ArchiveError, Result};
use crate::extractor::ArchiveEntry;
use crate::format::ArchiveFormat;
use crate::path_safety::validate_path;
use std::io::{Cursor, Write};

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

    fn build_zip(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();

        for entry in entries {
            match entry {
                ArchiveEntry::File { path, data } => {
                    validate_path(path, self.allow_unsafe_path_traversals)?;
                    writer.start_file(path, options)?;
                    writer.write_all(data)?;
                }
                ArchiveEntry::Directory { path } => {
                    validate_path(path, self.allow_unsafe_path_traversals)?;
                    writer.add_directory(path.clone(), options)?;
                }
                ArchiveEntry::Symlink { path, target } => {
                    validate_path(path, self.allow_unsafe_path_traversals)?;
                    validate_path(target, self.allow_unsafe_path_traversals)?;
                    writer.add_symlink(path, target, options)?;
                }
            }
        }

        Ok(writer.finish()?.into_inner())
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
                ArchiveEntry::File { path, data } => {
                    validate_path(path, allow_unsafe_path_traversals)?;
                    let mut header = tar::Header::new_gnu();
                    header.set_size(data.len() as u64);
                    header.set_mode(0o644);
                    header.set_cksum();
                    builder.append_data(&mut header, path, data.as_slice())?;
                }
                ArchiveEntry::Directory { path } => {
                    validate_path(path, allow_unsafe_path_traversals)?;
                    let mut header = tar::Header::new_gnu();
                    header.set_size(0);
                    header.set_mode(0o755);
                    header.set_entry_type(tar::EntryType::Directory);
                    header.set_cksum();
                    builder.append_data(&mut header, path, std::io::empty())?;
                }
                ArchiveEntry::Symlink { path, target } => {
                    validate_path(path, allow_unsafe_path_traversals)?;
                    validate_path(target, allow_unsafe_path_traversals)?;
                    let mut header = tar::Header::new_gnu();
                    header.set_size(0);
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
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        Self::write_tar_entries(&mut builder, entries, self.allow_unsafe_path_traversals)?;
        Ok(builder.into_inner()?.finish()?)
    }

    fn build_tar_bz2(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        Self::write_tar_entries(&mut builder, entries, self.allow_unsafe_path_traversals)?;
        Ok(builder.into_inner()?.finish()?)
    }

    fn build_tar_zst(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        // Level 0 asks zstd for its own default (currently 3).
        let encoder = zstd::stream::write::Encoder::new(Vec::new(), 0)?;
        let mut builder = tar::Builder::new(encoder);
        Self::write_tar_entries(&mut builder, entries, self.allow_unsafe_path_traversals)?;
        Ok(builder.into_inner()?.finish()?)
    }

    fn build_tar_lz4(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let encoder = lz4::EncoderBuilder::new().build(Vec::new())?;
        let mut builder = tar::Builder::new(encoder);
        Self::write_tar_entries(&mut builder, entries, self.allow_unsafe_path_traversals)?;
        let (buf, result) = builder.into_inner()?.finish();
        result?;
        Ok(buf)
    }

    fn build_tar_xz(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        // lzma_rs only exposes a one-shot xz_compress(&mut R, &mut W), not
        // a streaming Write encoder, so the tar has to be fully built in
        // memory first and then compressed in a single pass.
        let mut builder = tar::Builder::new(Vec::new());
        Self::write_tar_entries(&mut builder, entries, self.allow_unsafe_path_traversals)?;
        let tar_bytes = builder.into_inner()?;

        let mut output = Vec::new();
        lzma_rs::xz_compress(&mut Cursor::new(tar_bytes), &mut output)
            .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;
        Ok(output)
    }

    fn build_ar(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let mut builder = ar::Builder::new(Vec::new());

        for entry in entries {
            match entry {
                ArchiveEntry::File { path, data } => {
                    validate_path(path, self.allow_unsafe_path_traversals)?;
                    let header = ar::Header::new(path.clone().into_bytes(), data.len() as u64);
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

    fn build_7z(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let mut writer = sevenz_rust::SevenZWriter::new(Cursor::new(Vec::new()))
            .map_err(|e| ArchiveError::InvalidArchive(format!("7z error: {}", e)))?;

        for entry in entries {
            match entry {
                ArchiveEntry::File { path, data } => {
                    validate_path(path, self.allow_unsafe_path_traversals)?;
                    let mut sz_entry = sevenz_rust::SevenZArchiveEntry::new();
                    sz_entry.name = path.clone();
                    sz_entry.has_stream = true;
                    writer
                        .push_archive_entry(sz_entry, Some(Cursor::new(data.clone())))
                        .map_err(|e| ArchiveError::InvalidArchive(format!("7z error: {}", e)))?;
                }
                ArchiveEntry::Directory { path } => {
                    validate_path(path, self.allow_unsafe_path_traversals)?;
                    let mut sz_entry = sevenz_rust::SevenZArchiveEntry::new();
                    sz_entry.name = path.clone();
                    sz_entry.is_directory = true;
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
    ) -> Result<(String, Vec<u8>)> {
        let [entry] = entries else {
            return Err(ArchiveError::UnsupportedFormat(format!(
                "single-file compressed archives must contain exactly one file entry, got {}",
                entries.len()
            )));
        };

        match entry {
            ArchiveEntry::File { path, data } => {
                validate_path(path, allow_unsafe_path_traversals)?;
                Ok((path.clone(), data.clone()))
            }
            ArchiveEntry::Directory { .. } | ArchiveEntry::Symlink { .. } => {
                Err(ArchiveError::UnsupportedFormat(
                    "single-file compressed archives can only contain a file entry".to_string(),
                ))
            }
        }
    }

    fn build_single_gz(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let (path, data) = Self::single_file_entry(entries, self.allow_unsafe_path_traversals)?;
        let header = flate2::GzBuilder::new().filename(path);
        let mut encoder = header.write(Vec::new(), flate2::Compression::default());
        encoder.write_all(&data)?;
        Ok(encoder.finish()?)
    }

    fn build_single_bz2(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let (_path, data) = Self::single_file_entry(entries, self.allow_unsafe_path_traversals)?;
        let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
        encoder.write_all(&data)?;
        Ok(encoder.finish()?)
    }

    fn build_single_xz(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let (_path, data) = Self::single_file_entry(entries, self.allow_unsafe_path_traversals)?;
        let mut output = Vec::new();
        lzma_rs::xz_compress(&mut Cursor::new(data), &mut output)
            .map_err(|e| ArchiveError::InvalidArchive(e.to_string()))?;
        Ok(output)
    }

    fn build_single_lz4(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let (_path, data) = Self::single_file_entry(entries, self.allow_unsafe_path_traversals)?;
        let mut encoder = lz4::EncoderBuilder::new().build(Vec::new())?;
        encoder.write_all(&data)?;
        let (buf, result) = encoder.finish();
        result?;
        Ok(buf)
    }

    fn build_single_zst(&self, entries: &[ArchiveEntry]) -> Result<Vec<u8>> {
        let (_path, data) = Self::single_file_entry(entries, self.allow_unsafe_path_traversals)?;
        let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 0)?;
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
