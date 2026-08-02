//! Tests for `ArchiveExtractor::extract_streaming`.

use archive::{ArchiveBuilder, ArchiveEntry, ArchiveError, ArchiveExtractor, ArchiveFormat};

/// Formats that support files, directories, and symlinks (mirrors
/// `builder_tests.rs`'s `FULL_FEATURED_FORMATS`).
const FULL_FEATURED_FORMATS: &[ArchiveFormat] = &[
    ArchiveFormat::Zip,
    ArchiveFormat::Tar,
    ArchiveFormat::TarGz,
    ArchiveFormat::TarBz2,
    ArchiveFormat::TarXz,
    ArchiveFormat::TarZst,
    ArchiveFormat::TarLz4,
];

const SINGLE_FILE_FORMATS: &[ArchiveFormat] = &[
    ArchiveFormat::Gz,
    ArchiveFormat::Bz2,
    ArchiveFormat::Xz,
    ArchiveFormat::Lz4,
    ArchiveFormat::Zst,
];

#[test]
fn streaming_round_trips_files_and_directories() {
    for &format in FULL_FEATURED_FORMATS {
        let entries = vec![
            ArchiveEntry::file("hello.txt", b"Hello, World!".to_vec()),
            ArchiveEntry::file("nested/deep/file.txt", b"nested content".to_vec()),
            ArchiveEntry::directory("empty-dir"),
        ];
        let bytes = ArchiveBuilder::new().build(&entries, format).unwrap();

        let mut seen: Vec<(String, bool, Vec<u8>)> = Vec::new();
        ArchiveExtractor::new()
            .extract_streaming(&bytes, format, |meta, reader| {
                let mut contents = Vec::new();
                reader.read_to_end(&mut contents).unwrap();
                seen.push((meta.path.clone(), meta.is_dir, contents));
                Ok(())
            })
            .unwrap_or_else(|e| panic!("{format:?} streaming failed: {e}"));

        let hello = seen
            .iter()
            .find(|(path, ..)| path == "hello.txt")
            .unwrap_or_else(|| panic!("hello.txt missing for {format:?}: {seen:?}"));
        assert_eq!(hello.2, b"Hello, World!", "{format:?}");

        let nested = seen
            .iter()
            .find(|(path, ..)| path == "nested/deep/file.txt")
            .unwrap_or_else(|| panic!("nested file missing for {format:?}: {seen:?}"));
        assert_eq!(nested.2, b"nested content", "{format:?}");

        assert!(
            seen.iter()
                .any(|(path, is_dir, _)| path.trim_end_matches('/') == "empty-dir" && *is_dir),
            "expected empty-dir directory entry for {format:?}, got {seen:?}"
        );
    }
}

#[test]
fn streaming_round_trips_symlinks() {
    for &format in FULL_FEATURED_FORMATS {
        let entries = vec![
            ArchiveEntry::file("target.txt", b"real content".to_vec()),
            ArchiveEntry::symlink("link.txt", "target.txt"),
        ];
        let bytes = ArchiveBuilder::new().build(&entries, format).unwrap();

        let mut link_target = None;
        ArchiveExtractor::new()
            .extract_streaming(&bytes, format, |meta, _reader| {
                if meta.path == "link.txt" {
                    assert!(meta.is_symlink, "{format:?}: {meta:?}");
                    link_target = meta.symlink_target().map(str::to_string);
                }
                Ok(())
            })
            .unwrap_or_else(|e| panic!("{format:?} streaming failed: {e}"));

        assert_eq!(
            link_target.as_deref(),
            Some("target.txt"),
            "{format:?}"
        );
    }
}

#[test]
fn streaming_round_trips_single_file_formats() {
    for &format in SINGLE_FILE_FORMATS {
        let entries = vec![ArchiveEntry::file("hello.txt", b"Hello, World!".to_vec())];
        let bytes = ArchiveBuilder::new().build(&entries, format).unwrap();

        let mut contents = Vec::new();
        let mut count = 0;
        ArchiveExtractor::new()
            .extract_streaming(&bytes, format, |meta, reader| {
                assert!(meta.is_file(), "{format:?}: {meta:?}");
                reader.read_to_end(&mut contents).unwrap();
                count += 1;
                Ok(())
            })
            .unwrap_or_else(|e| panic!("{format:?} streaming failed: {e}"));

        assert_eq!(count, 1, "{format:?}");
        assert_eq!(contents, b"Hello, World!", "{format:?}");
    }
}

#[test]
fn streaming_ar_and_deb_round_trip() {
    let entries = vec![
        ArchiveEntry::file("hello.txt", b"Hello, World!".to_vec()),
        ArchiveEntry::file("other.txt", b"other content".to_vec()),
    ];

    for format in [ArchiveFormat::Ar, ArchiveFormat::Deb] {
        let bytes = ArchiveBuilder::new().build(&entries, format).unwrap();

        let mut seen = Vec::new();
        ArchiveExtractor::new()
            .extract_streaming(&bytes, format, |meta, reader| {
                let mut contents = Vec::new();
                reader.read_to_end(&mut contents).unwrap();
                seen.push((meta.path.clone(), contents));
                Ok(())
            })
            .unwrap();

        assert_eq!(seen.len(), 2, "{format:?}: {seen:?}");
        let hello = seen.iter().find(|(path, _)| path == "hello.txt").unwrap();
        assert_eq!(hello.1, b"Hello, World!");
    }
}

#[test]
fn streaming_sevenz_round_trips_files_and_directories() {
    let entries = vec![
        ArchiveEntry::file("hello.txt", b"Hello, World!".to_vec()),
        ArchiveEntry::directory("empty-dir"),
    ];
    let bytes = ArchiveBuilder::new()
        .build(&entries, ArchiveFormat::SevenZ)
        .unwrap();

    let mut seen: Vec<(String, bool, Vec<u8>)> = Vec::new();
    ArchiveExtractor::new()
        .extract_streaming(&bytes, ArchiveFormat::SevenZ, |meta, reader| {
            let mut contents = Vec::new();
            reader.read_to_end(&mut contents).unwrap();
            seen.push((meta.path.clone(), meta.is_dir, contents));
            Ok(())
        })
        .unwrap();

    let hello = seen.iter().find(|(path, ..)| path == "hello.txt").unwrap();
    assert_eq!(hello.2, b"Hello, World!");
    assert!(seen.iter().any(|(path, is_dir, _)| path == "empty-dir" && *is_dir));
}

#[test]
fn streaming_enforces_max_file_size() {
    let entries = vec![ArchiveEntry::file("big.txt", vec![0u8; 10_000])];

    for &format in FULL_FEATURED_FORMATS {
        let bytes = ArchiveBuilder::new().build(&entries, format).unwrap();

        let err = ArchiveExtractor::new()
            .with_max_file_size(1024)
            .extract_streaming(&bytes, format, |_meta, reader| {
                let mut sink = Vec::new();
                std::io::copy(reader, &mut sink).map_err(ArchiveError::Io)?;
                Ok(())
            })
            .unwrap_err();

        assert!(matches!(err, ArchiveError::FileTooLarge { .. }), "{format:?}: {err:?}");
    }
}

#[test]
fn streaming_enforces_max_total_size() {
    let entries = vec![
        ArchiveEntry::file("a.txt", vec![0u8; 6_000]),
        ArchiveEntry::file("b.txt", vec![0u8; 6_000]),
    ];

    for &format in FULL_FEATURED_FORMATS {
        let bytes = ArchiveBuilder::new().build(&entries, format).unwrap();

        let err = ArchiveExtractor::new()
            .with_max_file_size(1024 * 1024)
            .with_max_total_size(10_000)
            .extract_streaming(&bytes, format, |_meta, reader| {
                let mut sink = Vec::new();
                std::io::copy(reader, &mut sink).map_err(ArchiveError::Io)?;
                Ok(())
            })
            .unwrap_err();

        assert!(matches!(err, ArchiveError::TotalSizeTooLarge { .. }), "{format:?}: {err:?}");
    }
}

#[test]
fn streaming_rejects_unsafe_entry_path() {
    let entries = vec![ArchiveEntry::file("../../etc/passwd", b"pwned".to_vec())];
    let bytes = ArchiveBuilder::new()
        .allow_unsafe_path_traversals(true)
        .build(&entries, ArchiveFormat::Zip)
        .unwrap();

    let err = ArchiveExtractor::new()
        .extract_streaming(&bytes, ArchiveFormat::Zip, |_meta, _reader| Ok(()))
        .unwrap_err();
    assert!(matches!(err, ArchiveError::UnsafePath(_)), "{err:?}");
}

#[test]
fn streaming_propagates_callback_error() {
    let entries = vec![ArchiveEntry::file("hello.txt", b"Hello, World!".to_vec())];
    let bytes = ArchiveBuilder::new()
        .build(&entries, ArchiveFormat::Zip)
        .unwrap();

    let err = ArchiveExtractor::new()
        .extract_streaming(&bytes, ArchiveFormat::Zip, |_meta, _reader| {
            Err(ArchiveError::InvalidArchive("callback bailed".to_string()))
        })
        .unwrap_err();
    assert!(matches!(err, ArchiveError::InvalidArchive(msg) if msg == "callback bailed"));
}

#[test]
fn streaming_does_not_call_back_for_directories_with_data() {
    let entries = vec![ArchiveEntry::directory("empty-dir")];
    let bytes = ArchiveBuilder::new()
        .build(&entries, ArchiveFormat::Zip)
        .unwrap();

    ArchiveExtractor::new()
        .extract_streaming(&bytes, ArchiveFormat::Zip, |meta, reader| {
            assert!(meta.is_dir);
            let mut buf = [0u8; 1];
            assert_eq!(reader.read(&mut buf).unwrap(), 0, "directory reader should be empty");
            Ok(())
        })
        .unwrap();
}
