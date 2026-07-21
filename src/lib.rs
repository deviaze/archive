//! A unified interface for extracting and creating common archive formats in-memory.
//!
//! This crate provides a simple, safe API for extracting and creating various
//! archive formats including ZIP, TAR (with multiple compression options),
//! 7-Zip, ar/deb, and single-file compression formats. Everything happens
//! in-memory without touching the disk.
//! 
//! This also means this crate isn't great for streaming really big archives.
//! It's more of an ergonomic wrapper for applications that want usage safety
//! and don't want to import and handle all the deps themselves.
//!
//! # Features
//!
//! - **Unified API**: Single interface for extracting and creating all supported archive formats
//! - **In-memory**: No disk I/O required
//! - **Safety limits**: Protection against zip bombs, path traversal attacks, and resource exhaustion
//! - **Almost all Rust**: Minimal C dependencies (only bzip2)
//! - **Cross-platform**: Works on Linux, macOS, Windows (x86_64, ARM64)
//!
//! # Supported Formats
//!
//! Every format below can be both extracted ([`ArchiveExtractor`]) and
//! created ([`ArchiveBuilder`]), with two narrow exceptions noted on
//! [`ArchiveBuilder`] itself: `.7z` symlinks, and directories/symlinks in
//! `.ar`/`.deb` (the ar format has no concept of either).
//!
//! - **ZIP** (`.zip`)
//! - **TAR** (`.tar`, `.tar.gz`, `.tar.bz2`, `.tar.xz`, `.tar.zst`, `.tar.lz4`)
//! - **7-Zip** (`.7z`)
//! - **ar / Debian packages** (`.ar`, `.deb`)
//! - **Single-file compression** (`.gz`, `.bz2`, `.xz`, `.lz4`, `.zst`)
//!
//! # Examples
//!
//! ## Basic Usage
//!
//! ```no_run
//! use archive::{ArchiveExtractor, ArchiveFormat};
//! use std::fs;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Read archive file
//! let data = fs::read("example.zip")?;
//!
//! // Create extractor with default settings
//! let extractor = ArchiveExtractor::new();
//!
//! // Extract all files
//! let files = extractor.extract(&data, ArchiveFormat::Zip)?;
//!
//! // Process extracted files
//! for entry in &files {
//!     if let archive::ArchiveEntry::File { path, data, .. } = entry {
//!         println!("File: {} ({} bytes)", path, data.len());
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Creating Archives
//!
//! ```no_run
//! use archive::{ArchiveBuilder, ArchiveEntry, ArchiveFormat};
//! use std::fs;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let entries = vec![
//!     ArchiveEntry::file("hello.txt", b"Hello, World!".to_vec()),
//!     ArchiveEntry::directory("empty-dir"),
//! ];
//!
//! let bytes = ArchiveBuilder::new().build(&entries, ArchiveFormat::TarGz)?;
//! fs::write("example.tar.gz", bytes)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Custom Size Limits
//!
//! Protect against zip bombs and resource exhaustion:
//!
//! ```no_run
//! use archive::{ArchiveExtractor, ArchiveFormat};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let data = vec![0u8; 100];
//! let extractor = ArchiveExtractor::new()
//!     .with_max_file_size(50 * 1024 * 1024)      // 50 MB per file
//!     .with_max_total_size(500 * 1024 * 1024);   // 500 MB total
//!
//! let files = extractor.extract(&data, ArchiveFormat::Zip)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Multiple Archive Formats
//!
//! ```no_run
//! use archive::{ArchiveExtractor, ArchiveFormat};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let extractor = ArchiveExtractor::new();
//!
//! // Extract ZIP archive
//! # let zip_data = vec![0u8; 100];
//! let zip_files = extractor.extract(&zip_data, ArchiveFormat::Zip)?;
//!
//! // Extract TAR.GZ archive
//! # let targz_data = vec![0u8; 100];
//! let tar_files = extractor.extract(&targz_data, ArchiveFormat::TarGz)?;
//!
//! // Extract 7-Zip archive
//! # let sevenz_data = vec![0u8; 100];
//! let seven_files = extractor.extract(&sevenz_data, ArchiveFormat::SevenZ)?;
//!
//! // Decompress single gzip file
//! # let gz_data = vec![0u8; 100];
//! let gz_files = extractor.extract(&gz_data, ArchiveFormat::Gz)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Safety
//!
//! [`ArchiveExtractor`] includes built-in protections against:
//! - **Zip bombs**: Files that expand to enormous sizes
//! - **Resource exhaustion**: Configurable size limits
//! - **Path traversal**: Safe handling of archive paths
//!
//! Default limits:
//! - Maximum file size: 100 MB
//! - Maximum total extraction size: 1 GB
//!
//! [`ArchiveBuilder`] validates every entry's path (and, for symlinks, its
//! target) the same way extraction does, so an archive assembled from
//! less-trusted filenames can't come out containing `..` or absolute paths.
//!
//! # Error Handling
//!
//! ```no_run
//! use archive::{ArchiveExtractor, ArchiveFormat, ArchiveError};
//!
//! # fn main() {
//! let extractor = ArchiveExtractor::new()
//!     .with_max_file_size(1024 * 1024); // 1 MB limit
//!
//! # let data = vec![0u8; 100];
//! match extractor.extract(&data, ArchiveFormat::Zip) {
//!     Ok(files) => println!("Extracted {} files", files.len()),
//!     Err(ArchiveError::FileTooLarge { size, limit, .. }) => {
//!         eprintln!("File too large: {} bytes (limit: {})", size, limit);
//!     }
//!     Err(e) => eprintln!("Extraction failed: {}", e),
//! }
//! # }
//! ```

pub mod builder;
pub mod error;
pub mod extractor;
pub mod format;
pub mod path_safety;

pub use builder::ArchiveBuilder;
pub use error::{ArchiveError, Result};
pub use extractor::{ArchiveEntry, ArchiveExtractor};
pub use format::ArchiveFormat;
