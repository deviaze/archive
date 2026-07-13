//! Tests for archive creation via `ArchiveBuilder`.

use archive::{ArchiveBuilder, ArchiveEntry, ArchiveError, ArchiveExtractor, ArchiveFormat};

/// Formats that support files, directories, and symlinks (everything but
/// the single-file compressed formats, `.ar`/`.deb`, and `.7z`, which each
/// have their own narrower round-trip tests below).
const FULL_FEATURED_FORMATS: &[ArchiveFormat] = &[
    ArchiveFormat::Zip,
    ArchiveFormat::Tar,
    ArchiveFormat::TarGz,
    ArchiveFormat::TarBz2,
    ArchiveFormat::TarXz,
    ArchiveFormat::TarZst,
    ArchiveFormat::TarLz4,
];

fn round_trip(format: ArchiveFormat) {
    let entries = vec![
        ArchiveEntry::file("hello.txt", b"Hello, World!".to_vec()),
        ArchiveEntry::file("nested/deep/file.txt", b"nested content".to_vec()),
        ArchiveEntry::directory("empty-dir"),
    ];

    let bytes = ArchiveBuilder::new().build(&entries, format).unwrap();
    let extracted = ArchiveExtractor::new().extract(&bytes, format).unwrap();

    let hello = extracted
        .iter()
        .find(|e| e.path() == "hello.txt")
        .unwrap_or_else(|| panic!("hello.txt missing for {format:?}: {extracted:?}"));
    assert_eq!(hello.data(), Some(&b"Hello, World!"[..]));

    let nested = extracted
        .iter()
        .find(|e| e.path() == "nested/deep/file.txt")
        .unwrap_or_else(|| panic!("nested file missing for {format:?}: {extracted:?}"));
    assert_eq!(nested.data(), Some(&b"nested content"[..]));

    assert!(
        extracted
            .iter()
            .any(|e| e.path().trim_end_matches('/') == "empty-dir" && e.is_directory()),
        "expected empty-dir directory entry for {format:?}, got {extracted:?}"
    );
}

fn symlink_round_trip(format: ArchiveFormat) {
    let entries = vec![
        ArchiveEntry::file("target.txt", b"real content".to_vec()),
        ArchiveEntry::symlink("link.txt", "target.txt"),
    ];

    let bytes = ArchiveBuilder::new().build(&entries, format).unwrap();
    let extracted = ArchiveExtractor::new().extract(&bytes, format).unwrap();

    let link = extracted
        .iter()
        .find(|e| e.path() == "link.txt")
        .unwrap_or_else(|| panic!("link.txt missing for {format:?}: {extracted:?}"));
    assert!(link.is_symlink(), "{format:?}: {link:?}");
    assert!(
        matches!(link, ArchiveEntry::Symlink { target, .. } if target == "target.txt"),
        "{format:?}: {link:?}"
    );
}

#[test]
fn full_featured_formats_round_trip_files_and_directories() {
    for &format in FULL_FEATURED_FORMATS {
        round_trip(format);
    }
}

#[test]
fn full_featured_formats_round_trip_symlinks() {
    for &format in FULL_FEATURED_FORMATS {
        symlink_round_trip(format);
    }
}

#[test]
fn ar_round_trip() {
    let entries = vec![
        ArchiveEntry::file("hello.txt", b"Hello, World!".to_vec()),
        ArchiveEntry::file("other.txt", b"other content".to_vec()),
    ];

    for format in [ArchiveFormat::Ar, ArchiveFormat::Deb] {
        let bytes = ArchiveBuilder::new().build(&entries, format).unwrap();
        let extracted = ArchiveExtractor::new().extract(&bytes, format).unwrap();

        assert_eq!(extracted.len(), 2, "{format:?}: {extracted:?}");
        let hello = extracted.iter().find(|e| e.path() == "hello.txt").unwrap();
        assert_eq!(hello.data(), Some(&b"Hello, World!"[..]));
    }
}

#[test]
fn ar_rejects_directory_entries() {
    let entries = vec![ArchiveEntry::directory("some-dir")];
    let err = ArchiveBuilder::new()
        .build(&entries, ArchiveFormat::Ar)
        .unwrap_err();
    assert!(matches!(err, ArchiveError::UnsupportedFormat(_)), "{err:?}");
}

#[test]
fn ar_rejects_symlink_entries() {
    let entries = vec![ArchiveEntry::symlink("link", "target")];
    let err = ArchiveBuilder::new()
        .build(&entries, ArchiveFormat::Deb)
        .unwrap_err();
    assert!(matches!(err, ArchiveError::UnsupportedFormat(_)), "{err:?}");
}

#[test]
fn sevenz_round_trip() {
    let entries = vec![
        ArchiveEntry::file("hello.txt", b"Hello, World!".to_vec()),
        ArchiveEntry::directory("empty-dir"),
    ];

    let bytes = ArchiveBuilder::new()
        .build(&entries, ArchiveFormat::SevenZ)
        .unwrap();
    let extracted = ArchiveExtractor::new()
        .extract(&bytes, ArchiveFormat::SevenZ)
        .unwrap();

    let hello = extracted.iter().find(|e| e.path() == "hello.txt").unwrap();
    assert_eq!(hello.data(), Some(&b"Hello, World!"[..]));
    assert!(
        extracted
            .iter()
            .any(|e| e.path() == "empty-dir" && e.is_directory()),
        "{extracted:?}"
    );
}

#[test]
fn sevenz_rejects_symlink_entries() {
    let entries = vec![ArchiveEntry::symlink("link", "target")];
    let err = ArchiveBuilder::new()
        .build(&entries, ArchiveFormat::SevenZ)
        .unwrap_err();
    assert!(matches!(err, ArchiveError::UnsupportedFormat(_)), "{err:?}");
}

#[test]
fn single_file_formats_round_trip() {
    for format in [
        ArchiveFormat::Gz,
        ArchiveFormat::Bz2,
        ArchiveFormat::Xz,
        ArchiveFormat::Lz4,
        ArchiveFormat::Zst,
    ] {
        let entries = vec![ArchiveEntry::file("hello.txt", b"Hello, World!".to_vec())];
        let bytes = ArchiveBuilder::new().build(&entries, format).unwrap();
        let extracted = ArchiveExtractor::new().extract(&bytes, format).unwrap();

        assert_eq!(extracted.len(), 1, "{format:?}: {extracted:?}");
        assert_eq!(
            extracted[0].data(),
            Some(&b"Hello, World!"[..]),
            "{format:?}"
        );
    }
}

#[test]
fn single_file_formats_reject_multiple_entries() {
    let entries = vec![
        ArchiveEntry::file("a.txt", b"a".to_vec()),
        ArchiveEntry::file("b.txt", b"b".to_vec()),
    ];
    let err = ArchiveBuilder::new()
        .build(&entries, ArchiveFormat::Gz)
        .unwrap_err();
    assert!(matches!(err, ArchiveError::UnsupportedFormat(_)), "{err:?}");
}

#[test]
fn single_file_formats_reject_directory_entries() {
    let entries = vec![ArchiveEntry::directory("some-dir")];
    let err = ArchiveBuilder::new()
        .build(&entries, ArchiveFormat::Zst)
        .unwrap_err();
    assert!(matches!(err, ArchiveError::UnsupportedFormat(_)), "{err:?}");
}

#[test]
fn build_single_file_round_trips_without_wrapping_in_a_vec() {
    for format in [
        ArchiveFormat::Gz,
        ArchiveFormat::Bz2,
        ArchiveFormat::Xz,
        ArchiveFormat::Lz4,
        ArchiveFormat::Zst,
    ] {
        let bytes = ArchiveBuilder::new()
            .build_single_file("hello.txt", b"Hello, World!".to_vec(), format)
            .unwrap();
        let extracted = ArchiveExtractor::new().extract(&bytes, format).unwrap();

        assert_eq!(extracted.len(), 1, "{format:?}: {extracted:?}");
        assert_eq!(
            extracted[0].data(),
            Some(&b"Hello, World!"[..]),
            "{format:?}"
        );
    }
}

#[test]
fn build_single_file_rejects_container_formats() {
    let err = ArchiveBuilder::new()
        .build_single_file("hello.txt", b"Hello, World!".to_vec(), ArchiveFormat::Zip)
        .unwrap_err();
    assert!(matches!(err, ArchiveError::UnsupportedFormat(_)), "{err:?}");
}

#[test]
fn build_rejects_unsafe_entry_path() {
    let entries = vec![ArchiveEntry::file("../../etc/passwd", b"pwned".to_vec())];

    let err = ArchiveBuilder::new()
        .build(&entries, ArchiveFormat::Zip)
        .unwrap_err();
    assert!(matches!(err, ArchiveError::UnsafePath(_)), "{err:?}");
}

#[test]
fn build_rejects_unsafe_symlink_target() {
    let entries = vec![ArchiveEntry::symlink("link.txt", "../../etc/passwd")];

    let err = ArchiveBuilder::new()
        .build(&entries, ArchiveFormat::TarGz)
        .unwrap_err();
    assert!(matches!(err, ArchiveError::UnsafePath(_)), "{err:?}");
}

#[test]
#[allow(clippy::unnecessary_to_owned)] // exercising String as an accepted input type, not just &str
fn file_constructor_accepts_path_like_types() {
    use std::path::{Path, PathBuf};

    let from_str = ArchiveEntry::file("a.txt", b"a".to_vec());
    let from_string = ArchiveEntry::file("b.txt".to_string(), b"b".to_vec());
    let from_path = ArchiveEntry::file(Path::new("c.txt"), b"c".to_vec());
    let from_pathbuf = ArchiveEntry::file(PathBuf::from("d.txt"), b"d".to_vec());

    assert_eq!(from_str.path(), "a.txt");
    assert_eq!(from_string.path(), "b.txt");
    assert_eq!(from_path.path(), "c.txt");
    assert_eq!(from_pathbuf.path(), "d.txt");
}

#[test]
fn file_constructor_normalizes_nested_path_components() {
    use std::path::PathBuf;

    let entry = ArchiveEntry::file(PathBuf::from("a").join("b").join("c.txt"), b"x".to_vec());
    // PathBuf::join always uses '/' as the in-memory separator on Unix;
    // this mainly documents that the constructor doesn't mangle nested
    // paths built up via the Path API rather than string concatenation.
    assert_eq!(entry.path(), "a/b/c.txt");
}
