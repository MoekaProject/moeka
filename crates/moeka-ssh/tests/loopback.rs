#![cfg(unix)]

use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use moeka_fs::backend::{ExpectedRevision, Filesystem, SaveOutcome, SaveRequest};
use moeka_fs::local::native as fs;
use moeka_fs::path::NativePath;
use moeka_fs::search::SearchPathFilters;
use moeka_fs::service::{Request, Response, SearchItem, SearchOptions};
use moeka_ssh::filesystem::SshFilesystem;
use moeka_ssh::{ServerPayload, SshOptions, detect_target, install};
use sha2::{Digest, Sha256};

#[test]
#[ignore = "requires an isolated loopback sshd, disposable HOME, and native server binary"]
fn loopback_install_reuse_files_search_watch_and_restart_outcomes() {
    let config =
        PathBuf::from(std::env::var_os("MOEKA_LOOPBACK_CONFIG").expect("loopback SSH config"));
    let home = std::env::var("MOEKA_LOOPBACK_HOME").expect("disposable remote HOME");
    let binary =
        PathBuf::from(std::env::var_os("MOEKA_LOOPBACK_SERVER").expect("native server binary"));
    let destination = std::env::var("MOEKA_LOOPBACK_DESTINATION")
        .unwrap_or_else(|_| "moeka-loopback-test".into());
    let options = SshOptions {
        destination,
        port: None,
        config_file: Some(config),
    };
    let output = options
        .command()
        .unwrap()
        .arg("printf '%s' \"$HOME\"")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        home,
        "remote HOME must be disposable"
    );
    let bytes = fs::read(&binary).unwrap();
    let target = detect_target(&options).unwrap();
    let version = format!("loopback-{}", std::process::id());
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let payload = ServerPayload {
        target,
        version: &version,
        sha256: &digest,
        bytes: &bytes,
    };
    assert!(install(&options, &payload).unwrap());
    assert!(!install(&options, &payload).unwrap());
    let remote = SshFilesystem::connect(&options, &payload, None).unwrap();
    assert!(
        remote
            .capabilities()
            .iter()
            .any(|capability| capability == "page_credit_window")
    );
    let requested_root_text = format!("{home}/workspace-{}", std::process::id());
    let requested_root = NativePath::Unix(requested_root_text.as_bytes().to_vec());
    for protected in [
        NativePath::Unix(b"/".to_vec()),
        NativePath::Unix(home.as_bytes().to_vec()),
    ] {
        assert_eq!(remote.trash(&protected).unwrap_err().kind, "InvalidInput");
    }
    remote.create_dir(&requested_root, true).unwrap();
    let root = remote.canonicalize(&requested_root).unwrap();
    let root_text = root.to_string();
    let file = NativePath::Unix(format!("{root_text}/note.md").into_bytes());
    assert_eq!(
        root.to_local().unwrap(),
        fs::canonicalize(&requested_root_text).unwrap()
    );
    let text = "first needle\nsecond needle\n";
    let request = SaveRequest {
        operation_id: "loopback-save-1".into(),
        path: file.clone(),
        expected: ExpectedRevision::Missing,
        bytes: text.as_bytes().to_vec(),
    };
    let outcome = remote.save(request.clone()).unwrap();
    assert!(matches!(outcome, SaveOutcome::Saved(_)));
    assert_eq!(remote.save(request).unwrap(), outcome);
    assert_eq!(remote.save_outcome("loopback-save-1"), outcome);
    let background = SshFilesystem::connect(&options, &payload, None).unwrap();
    remote.verify_peer(&background).unwrap();
    assert_eq!(
        background.save_outcome("loopback-save-1"),
        SaveOutcome::Unknown,
        "paired channels must not be used interchangeably for save journals"
    );
    assert_eq!(remote.save_outcome("loopback-save-1"), outcome);
    drop(background);
    std::os::unix::fs::symlink("note.md", format!("{root_text}/relative.md")).unwrap();
    assert_eq!(
        remote
            .read_link(&NativePath::Unix(
                format!("{root_text}/relative.md").into_bytes()
            ))
            .unwrap(),
        NativePath::Unix(b"note.md".to_vec())
    );
    assert_eq!(
        fs::read_to_string(format!("{root_text}/note.md")).unwrap(),
        text
    );
    let missing = NativePath::Unix(format!("{root_text}/missing.md").into_bytes());
    let metadata = remote
        .metadata_batch(&[file.clone(), missing.clone()], true)
        .unwrap();
    assert_eq!(metadata[0].as_ref().unwrap().len, text.len() as u64);
    assert_eq!(metadata[1].as_ref().unwrap_err().kind, "NotFound");
    let read = remote.read_chunk(&file, 0, 64).unwrap();
    assert!(read.eof);
    assert_eq!(read.bytes, text.as_bytes());
    let previews = remote.read_previews(&[file.clone(), missing], 7).unwrap();
    assert_eq!(previews[0].as_ref().unwrap().bytes, &text.as_bytes()[..7]);
    assert_eq!(previews[1].as_ref().unwrap_err().kind, "NotFound");
    fs::write(format!("{root_text}/excluded.md"), [255]).unwrap();
    let Response::Session(search) = remote
        .request(Request::StartFilteredSearch {
            root: root.clone(),
            filters: SearchPathFilters {
                include: r"\.md$".into(),
                exclude: r"^excluded\.md$".into(),
            },
            options: SearchOptions {
                query: "needle".into(),
                case_sensitive: true,
                whole_word: false,
                regex: false,
            },
        })
        .unwrap()
    else {
        panic!("expected search session");
    };
    let mut matches = Vec::new();
    loop {
        let (items, done) = remote.next_search_pages(search).unwrap();
        for item in items {
            match item {
                SearchItem::Match {
                    start, end, path, ..
                } => {
                    assert_eq!(path, file);
                    matches.push((start, end));
                }
                SearchItem::Error(error) => panic!("search failure: {error:?}"),
            }
        }
        if done {
            break;
        }
    }
    assert_eq!(matches, vec![(6, 12), (20, 26)]);
    fs::write(format!("{root_text}/picture.png"), b"needle").unwrap();
    fs::write(format!("{root_text}/pdf"), b"%PDF-1.7\nneedle").unwrap();
    fs::write(format!("{root_text}/binary"), b"needle\0").unwrap();
    fs::write(format!("{root_text}/lossy.md"), b"\xffneedle").unwrap();
    let Response::Session(search) = remote
        .request(Request::StartSearch {
            root: root.clone(),
            options: SearchOptions {
                query: "needle".into(),
                case_sensitive: true,
                whole_word: false,
                regex: false,
            },
        })
        .unwrap()
    else {
        panic!("expected search session");
    };
    let mut found = Vec::new();
    loop {
        let (items, done) = remote.next_search_pages(search).unwrap();
        for item in items {
            match item {
                SearchItem::Match {
                    path, start, end, ..
                } => found.push((path, start, end)),
                SearchItem::Error(error) => panic!("search failure: {error}"),
            }
        }
        if done {
            break;
        }
    }
    assert_eq!(found.len(), 3);
    assert!(found.contains(&(
        NativePath::Unix(format!("{root_text}/lossy.md").into_bytes()),
        1,
        7
    )));
    let Response::Session(watch) = remote
        .request(Request::StartWatch {
            root: root.clone(),
            generation: 7,
            recursive: true,
        })
        .unwrap()
    else {
        panic!("expected watch session");
    };
    let Response::Watch(initial) = remote
        .request(Request::PollWatch {
            session: watch,
            after_sequence: 0,
        })
        .unwrap()
    else {
        panic!("expected initial watch snapshot");
    };
    assert!(initial.invalidated);
    assert_eq!(initial.generation, 7);
    let new_file = NativePath::Unix(format!("{root_text}/changed.md").into_bytes());
    remote.create_file(&new_file).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut sequence = initial.sequence;
    loop {
        let Response::Watch(batch) = remote
            .request(Request::PollWatch {
                session: watch,
                after_sequence: sequence,
            })
            .unwrap()
        else {
            panic!("expected watch update");
        };
        assert_eq!(batch.generation, 7);
        assert!(batch.error.is_none());
        if batch.invalidated
            || batch
                .events
                .iter()
                .any(|event| event.paths.contains(&new_file))
        {
            break;
        }
        sequence = batch.sequence;
        assert!(
            Instant::now() < deadline,
            "native watcher did not report the mutation"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    remote.request(Request::Cancel { session: watch }).unwrap();
    let conflict = remote.rename(&new_file, &file).unwrap_err();
    assert_eq!(conflict.kind, "AlreadyExists");
    assert!(remote.metadata(&new_file, false).is_ok());
    let renamed = NativePath::Unix(format!("{root_text}/renamed.md").into_bytes());
    remote.rename(&new_file, &renamed).unwrap();
    assert_eq!(
        remote.metadata(&new_file, false).unwrap_err().kind,
        "NotFound"
    );

    let large_path = NativePath::Unix(format!("{root_text}/large.txt").into_bytes());
    let mut large_bytes = vec![b'a'; 5 * 1024 * 1024];
    large_bytes.extend_from_slice(b"\nlast needle\n");
    let save_started = Instant::now();
    assert!(matches!(
        remote
            .save(SaveRequest {
                operation_id: "large-streamed-save".into(),
                path: large_path.clone(),
                expected: ExpectedRevision::Missing,
                bytes: large_bytes.clone(),
            })
            .unwrap(),
        SaveOutcome::Saved(_)
    ));
    let save_elapsed = save_started.elapsed();
    let read_started = Instant::now();
    assert_eq!(
        remote.read_file(&large_path, large_bytes.len()).unwrap().0,
        large_bytes
    );
    eprintln!(
        "loopback {} bytes: save {:?}, read {:?}",
        large_bytes.len(),
        save_elapsed,
        read_started.elapsed()
    );
    let Response::Session(large_search) = remote
        .request(Request::StartSearch {
            root: large_path.clone(),
            options: SearchOptions {
                query: "NEEDLE".into(),
                case_sensitive: false,
                whole_word: true,
                regex: false,
            },
        })
        .unwrap()
    else {
        panic!("expected large search session")
    };
    let mut large_matches = Vec::new();
    loop {
        let (items, done) = remote.next_search_pages(large_search).unwrap();
        for item in items {
            match item {
                SearchItem::Match {
                    start, end, line, ..
                } => large_matches.push((start, end, line)),
                SearchItem::Error(error) => panic!("large search failed: {error}"),
            }
        }
        if done {
            break;
        }
    }
    assert_eq!(
        large_matches,
        [(5 * 1024 * 1024 + 6, 5 * 1024 * 1024 + 12, 1)]
    );
    assert!(
        remote
            .read_file(&large_path, large_bytes.len() - 1)
            .is_err()
    );
    for index in 0..140 {
        remote
            .create_file(&NativePath::Unix(
                format!("{root_text}/batch-{index}.txt").into_bytes(),
            ))
            .unwrap();
    }
    let Response::Session(directory) = remote
        .request(Request::OpenDirectory { path: root })
        .unwrap()
    else {
        panic!("expected directory session");
    };
    let mut entries = std::collections::HashSet::new();
    let mut pages = 0;
    loop {
        let (page, done) = remote.next_directory_pages(directory).unwrap();
        pages += 1;
        for entry in page {
            let entry = entry.unwrap();
            assert!(entry.metadata.is_ok());
            assert!(entries.insert(entry.path), "duplicate directory entry");
        }
        if done {
            break;
        }
    }
    assert!(pages >= 2, "directory test must cross a page boundary");
    assert_eq!(entries.len(), fs::read_dir(&root_text).unwrap().count());
    drop(remote);
    let reconnected = SshFilesystem::connect(&options, &payload, None).unwrap();
    assert_eq!(
        reconnected.save_outcome("loopback-save-1"),
        SaveOutcome::Unknown
    );
    assert_eq!(
        fs::read_to_string(format!("{root_text}/note.md")).unwrap(),
        text
    );
    drop(reconnected);
    let installed = PathBuf::from(&home)
        .join(".local/share/moeka/remote-server")
        .join(format!("{version}-{}-{digest}", target.name()))
        .join("moeka-remote-server");
    fs::write(&installed, b"corrupted test payload").unwrap();
    let error = install(&options, &payload).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert!(error.to_string().contains("integrity verification"));
    assert_eq!(fs::read(installed).unwrap(), b"corrupted test payload");
}
