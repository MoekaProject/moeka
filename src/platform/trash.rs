//! Local paths go to Trash; confirmed network volumes are deleted permanently.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context as _;

use crate::platform::fs_task::check_deadline;

pub(crate) fn move_to_trash(path: &Path) -> anyhow::Result<()> {
    delete_with(path, is_network_volume, trash_local)
}

fn delete_with(
    path: &Path,
    network_volume: impl FnOnce(&Path, &fs::Metadata) -> anyhow::Result<bool>,
    trash_local: impl FnOnce(&Path) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    // Resolve ancestors, not the final component: deleting a symlink must not
    // delete (or select a volume policy based on) its target.
    check_deadline()?;
    let path = deletion_path(path)?;
    check_deadline()?;
    let metadata = fs::symlink_metadata(&path)?;
    check_deadline()?;
    let is_network = network_volume(&path, &metadata)
        .with_context(|| crate::i18n::runtime::rt!(VolumeProbe, path.display()))?;
    // A probe can return after the caller has already observed a timeout.
    // Never let that late result start a destructive phase.
    check_deadline()?;
    if is_network {
        remove_permanently(&path)
            .with_context(|| crate::i18n::runtime::rt!(NetworkDeleteFailed, path.display()))
    } else {
        // Permission, sharing violations, aborted operations and unknown errors
        // are not evidence that a local file can safely bypass Trash.
        trash_local(&path).with_context(|| crate::i18n::runtime::rt!(TrashFailed, path.display()))
    }
}

fn deletion_path(path: &Path) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        !path.as_os_str().is_empty(),
        crate::i18n::runtime::RuntimeText::UnsafeDelete.to_string()
    );
    // absolute() can normalize "." and (on Windows) ".." into a deletable name.
    anyhow::ensure!(
        path.file_name().is_some(),
        crate::i18n::runtime::RuntimeText::UnsafeDelete.to_string()
    );
    let absolute = std::path::absolute(path)?;
    let name = absolute.file_name().ok_or_else(|| {
        anyhow::anyhow!(crate::i18n::runtime::RuntimeText::UnsafeDelete.to_string())
    })?;
    let parent = absolute.parent().ok_or_else(|| {
        anyhow::anyhow!(crate::i18n::runtime::RuntimeText::UnsafeDelete.to_string())
    })?;
    Ok(parent.canonicalize()?.join(name))
}

pub(crate) fn remove_permanently(path: &Path) -> std::io::Result<()> {
    check_deadline()?;
    let metadata = fs::symlink_metadata(path)?;
    let file_type = metadata.file_type();
    check_deadline()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;
        // Directory symlinks (including dangling ones) require RemoveDirectoryW.
        if file_type.is_symlink_dir() {
            return fs::remove_dir(path);
        }
    }
    if file_type.is_dir() && !file_type.is_symlink() {
        // std's implementation does not follow symlinks within the tree either.
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn volume_probe_path<'a>(path: &'a Path, metadata: &fs::Metadata) -> &'a Path {
    if metadata.file_type().is_symlink() {
        path.parent().expect("deletion_path validated the parent")
    } else {
        path
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn is_network_volume(path: &Path, metadata: &fs::Metadata) -> anyhow::Result<bool> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let probe = volume_probe_path(path, metadata);
    let probe = CString::new(probe.as_os_str().as_bytes())?;
    let mut status = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // statfs uses caller-owned storage, unlike getmntinfo's shared mount table.
    if unsafe { libc::statfs(probe.as_ptr(), status.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let status = unsafe { status.assume_init() };
    #[cfg(target_os = "macos")]
    {
        Ok(macos_mount_is_network(status.f_flags))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(linux_fs_is_network(status.f_type as u32 as u64))
    }
}

#[cfg(any(target_os = "macos", test))]
fn macos_mount_is_network(flags: u32) -> bool {
    // MNT_LOCAL (Darwin sys/mount.h); SMB/NFS mounts do not set this flag.
    flags & 0x0000_1000 == 0
}

#[cfg(any(target_os = "linux", test))]
fn linux_fs_is_network(fs_type: u64) -> bool {
    // Linux uapi/linux/magic.h: SMB, CIFS, SMB2, NFS, AFS, Coda, Ceph, 9P.
    // FUSE alone cannot distinguish a local filesystem from sshfs: fail closed.
    matches!(
        fs_type,
        0x517b
            | 0xff53_4d42
            | 0xfe53_4d42
            | 0x6969
            | 0x5346_414f
            | 0x7375_7245
            | 0x00c3_6400
            | 0x0102_1997
    )
}

#[cfg(windows)]
fn is_network_volume(path: &Path, metadata: &fs::Metadata) -> anyhow::Result<bool> {
    windows_network_volume(volume_probe_path(path, metadata), |probe| {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumePathNameW};

        let probe: Vec<u16> = probe.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut root = vec![0u16; 32768];
        if unsafe { GetVolumePathNameW(probe.as_ptr(), root.as_mut_ptr(), root.len() as u32) } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(unsafe { GetDriveTypeW(root.as_ptr()) })
    })
}

#[cfg(windows)]
fn windows_network_volume(
    path: &Path,
    drive_type: impl FnOnce(&Path) -> std::io::Result<u32>,
) -> anyhow::Result<bool> {
    use std::path::{Component, Prefix};

    if matches!(
        path.components().next(),
        Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..))
    ) {
        return Ok(true);
    }
    match drive_type(path)? {
        4 => Ok(true),              // DRIVE_REMOTE: includes mapped SMB drives.
        2 | 3 | 5 | 6 => Ok(false), // Removable, fixed, CD-ROM and RAM disk.
        _ => anyhow::bail!(crate::i18n::runtime::rt!(VolumeProbe, path.display())),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn is_network_volume(_path: &Path, _metadata: &fs::Metadata) -> anyhow::Result<bool> {
    Ok(false)
}

fn trash_local(path: &Path) -> anyhow::Result<()> {
    check_deadline()?;
    #[cfg(target_os = "macos")]
    {
        use trash::TrashContext;
        use trash::macos::{DeleteMethod, TrashContextExtMacos};

        // No Finder fallback: AppleEvents are forbidden in the App Store sandbox.
        let mut context = TrashContext::new();
        context.set_delete_method(DeleteMethod::NsFileManager);
        check_deadline()?;
        context.delete(path).map_err(anyhow::Error::from)
    }
    #[cfg(windows)]
    {
        let path = path.to_owned();
        run_trash_on_thread(move || trash::delete(path).map_err(anyhow::Error::from))
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        trash::delete(path).map_err(anyhow::Error::from)
    }
}

#[cfg(any(windows, test))]
fn run_trash_on_thread(
    operation: impl FnOnce() -> anyhow::Result<()> + Send + 'static,
) -> anyhow::Result<()> {
    // trash 5 initializes STA COM and can panic on CoInitializeEx/CoCreateInstance.
    // GPUI's WinRT pool may already be MTA; its FFI callback must not unwind.
    // A fresh Rust thread owns the COM apartment and join contains Rust panics.
    std::thread::Builder::new()
        .name("moeka-trash".into())
        .spawn(operation)
        .context(crate::i18n::runtime::RuntimeText::TrashFailed)?
        .join()
        .map_err(|panic| {
            let message = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("non-string panic");
            anyhow::anyhow!(crate::i18n::runtime::rt!(WorkerFailed, "Trash", message))
        })?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::Future,
        sync::Arc,
        task::{Poll, Wake, Waker},
        time::Duration,
    };

    struct TestWaker(std::thread::Thread);

    impl Wake for TestWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    fn block_on<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::from(Arc::new(TestWaker(std::thread::current())));
        let mut cx = std::task::Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            if let Poll::Ready(result) = future.as_mut().poll(&mut cx) {
                return result;
            }
            std::thread::park_timeout(Duration::from_millis(10));
        }
    }

    #[test]
    fn late_volume_probe_cannot_start_trash_or_permanent_deletion() {
        let temp = tempfile::tempdir().unwrap();
        crate::platform::fs_task::register_workspace_root(temp.path());
        for is_network in [true, false] {
            let file = temp.path().join(format!("note-{is_network}.md"));
            fs::write(&file, "keep").unwrap();
            let worker_file = file.clone();
            let (release, wait) = std::sync::mpsc::channel();
            let (finished, done) = std::sync::mpsc::channel();
            let result = block_on(crate::platform::fs_task::run_with_timeout(
                "test late volume probe",
                file.clone(),
                Duration::from_millis(100),
                move || {
                    let result = delete_with(
                        &worker_file,
                        |_, _| {
                            wait.recv_timeout(Duration::from_secs(2))?;
                            Ok(is_network)
                        },
                        |_| panic!("late volume probe must not invoke Trash"),
                    );
                    finished
                        .send(result.as_ref().is_err_and(|error| {
                            error
                                .downcast_ref::<std::io::Error>()
                                .is_some_and(|error| error.kind() == std::io::ErrorKind::TimedOut)
                        }))
                        .unwrap();
                    Ok(result)
                },
            ));
            assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
            release.send(()).unwrap();
            assert!(done.recv_timeout(Duration::from_secs(2)).unwrap());
            assert_eq!(fs::read_to_string(file).unwrap(), "keep");
        }
    }

    #[test]
    fn expired_permanent_deletion_keeps_the_source() {
        let temp = tempfile::tempdir().unwrap();
        crate::platform::fs_task::register_workspace_root(temp.path());
        let file = temp.path().join("note.md");
        fs::write(&file, "keep").unwrap();
        let worker_file = file.clone();
        let (release, wait) = std::sync::mpsc::channel();
        let (finished, done) = std::sync::mpsc::channel();
        let result = block_on(crate::platform::fs_task::run_with_timeout(
            "test expired permanent deletion",
            file.clone(),
            Duration::from_millis(100),
            move || {
                wait.recv_timeout(Duration::from_secs(2)).unwrap();
                let result = remove_permanently(&worker_file);
                finished
                    .send(result.as_ref().err().map(std::io::Error::kind))
                    .unwrap();
                result
            },
        ));
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
        release.send(()).unwrap();
        assert_eq!(
            done.recv_timeout(Duration::from_secs(2)).unwrap(),
            Some(std::io::ErrorKind::TimedOut)
        );
        assert_eq!(fs::read_to_string(file).unwrap(), "keep");
    }

    #[test]
    fn network_files_and_directories_are_removed_without_calling_trash() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("note.md");
        let directory = temp.path().join("notes");
        fs::write(&file, "note").unwrap();
        fs::create_dir_all(directory.join("nested")).unwrap();
        fs::write(directory.join("nested/child.md"), "child").unwrap();
        for path in [&file, &directory] {
            delete_with(
                path,
                |_, _| Ok(true),
                |_| panic!("network paths bypass Trash"),
            )
            .unwrap();
            assert!(fs::symlink_metadata(path).is_err());
        }
    }

    #[test]
    fn local_files_use_trash_without_direct_deletion() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("note.md");
        fs::write(&file, "keep").unwrap();
        let mut called = false;
        delete_with(
            &file,
            |_, _| Ok(false),
            |_| {
                called = true;
                Ok(())
            },
        )
        .unwrap();
        assert!(called);
        assert_eq!(fs::read_to_string(file).unwrap(), "keep");
    }

    #[test]
    fn local_trash_errors_never_allow_permanent_deletion() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("note.md");
        fs::write(&file, "keep").unwrap();
        let errors = [
            anyhow::Error::from(std::io::Error::from_raw_os_error(18)),
            anyhow::Error::from(std::io::Error::from_raw_os_error(45)),
            anyhow::Error::from(std::io::Error::from_raw_os_error(95)),
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "no trash").into(),
            std::io::Error::new(std::io::ErrorKind::Unsupported, "unsupported").into(),
            trash::Error::Os {
                code: 32,
                description: "file locked".into(),
            }
            .into(),
            trash::Error::Os {
                code: -2147024891,
                description: "access denied".into(),
            }
            .into(),
            trash::Error::Unknown {
                description: "Some operations were aborted".into(),
            }
            .into(),
            anyhow::anyhow!("trashItemAtURL: volume doesn't have one"),
            anyhow::anyhow!("废纸篓不可用 / recycle bin unavailable / .Trash: No such file"),
        ];
        for error in errors {
            assert!(delete_with(&file, |_, _| Ok(false), |_| Err(error)).is_err());
            assert_eq!(fs::read_to_string(&file).unwrap(), "keep");
        }
    }

    #[test]
    fn failed_volume_detection_does_not_delete_or_call_trash() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("note.md");
        fs::write(&file, "keep").unwrap();
        assert!(
            delete_with(
                &file,
                |_, _| Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "probe failed"
                )
                .into()),
                |_| panic!("failed probe must stop deletion"),
            )
            .is_err()
        );
        assert_eq!(fs::read_to_string(file).unwrap(), "keep");
    }

    #[test]
    fn invalid_and_missing_paths_do_not_reach_deletion() {
        let temp = tempfile::tempdir().unwrap();
        for path in [
            Path::new(""),
            Path::new("."),
            Path::new(".."),
            &temp.path().join(".."),
            temp.path().parent().unwrap().ancestors().last().unwrap(),
            &temp.path().join("missing"),
        ] {
            assert!(
                delete_with(
                    path,
                    |_, _| panic!("invalid target"),
                    |_| panic!("invalid target")
                )
                .is_err()
            );
        }
    }

    #[test]
    fn mount_flags_and_filesystem_types_are_not_error_message_dependent() {
        assert!(!macos_mount_is_network(0x1000));
        assert!(!macos_mount_is_network(0x1001));
        assert!(macos_mount_is_network(0));
        assert!(macos_mount_is_network(1));
        for fs_type in [
            0x517b,
            0xff53_4d42,
            0xfe53_4d42,
            0x6969,
            0x5346_414f,
            0x7375_7245,
            0x00c3_6400,
            0x0102_1997,
        ] {
            assert!(linux_fs_is_network(fs_type));
        }
        for fs_type in [0xef53, 0x9123_683e, 0x6573_5546] {
            assert!(!linux_fs_is_network(fs_type));
        }
    }

    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    #[test]
    fn native_volume_probe_accepts_a_temp_fixture() {
        let temp = tempfile::tempdir().unwrap();
        let path = deletion_path(temp.path()).unwrap();
        is_network_volume(&path, &fs::symlink_metadata(&path).unwrap()).unwrap();
    }

    #[test]
    fn trash_worker_has_its_own_thread_and_returns_errors() {
        let caller = std::thread::current().id();
        run_trash_on_thread(move || {
            assert_ne!(std::thread::current().id(), caller);
            Ok(())
        })
        .unwrap();
        let error = run_trash_on_thread(|| Err(anyhow::anyhow!("COM failure"))).unwrap_err();
        assert_eq!(error.to_string(), "COM failure");
    }

    #[cfg(windows)]
    #[test]
    fn trash_worker_can_initialize_sta_from_an_mta_caller() {
        use windows_sys::Win32::System::Com::{
            COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize,
        };

        struct Apartment;
        impl Drop for Apartment {
            fn drop(&mut self) {
                unsafe { CoUninitialize() };
            }
        }

        // Use a fresh caller too, so another test's apartment cannot interfere.
        std::thread::spawn(|| {
            assert!(unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) } >= 0);
            let _caller_apartment = Apartment;
            run_trash_on_thread(|| {
                assert!(
                    unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) }
                        >= 0
                );
                let _worker_apartment = Apartment;
                Ok(())
            })
            .unwrap();
        })
        .join()
        .unwrap();
    }

    #[test]
    fn trash_worker_contains_dependency_panics_without_deleting() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("note.md");
        fs::write(&file, "keep").unwrap();
        for operation in ["CoInitializeEx", "CoCreateInstance"] {
            let result = delete_with(
                &file,
                |_, _| Ok(false),
                |_| run_trash_on_thread(move || panic!("Call to {operation} failed")),
            );
            assert!(format!("{:#}", result.unwrap_err()).contains(operation));
            assert_eq!(fs::read_to_string(&file).unwrap(), "keep");
        }
        let error = run_trash_on_thread(|| std::panic::panic_any(())).unwrap_err();
        assert!(error.to_string().contains("non-string panic"));
        run_trash_on_thread(|| Ok(())).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn local_symlinks_keep_their_identity_through_ancestor_resolution() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("entries");
        let alias = temp.path().join("alias");
        let target = temp.path().join("target.md");
        fs::create_dir(&parent).unwrap();
        symlink(&parent, &alias).unwrap();
        fs::write(&target, "keep").unwrap();
        for destination in [&target, &temp.path().join("missing")] {
            let link = parent.join("link");
            symlink(destination, &link).unwrap();
            let expected = parent.canonicalize().unwrap().join("link");
            assert!(
                delete_with(
                    &alias.join("link"),
                    |path, metadata| {
                        assert_eq!(path, expected);
                        assert!(metadata.file_type().is_symlink());
                        assert_eq!(
                            volume_probe_path(path, metadata),
                            expected.parent().unwrap()
                        );
                        Ok(false)
                    },
                    |path| {
                        assert_eq!(path, expected);
                        Err(anyhow::anyhow!("unknown local Trash failure"))
                    },
                )
                .is_err()
            );
            assert_eq!(fs::read_link(&link).unwrap(), *destination);
            assert_eq!(fs::read_to_string(&target).unwrap(), "keep");
            fs::remove_file(link).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn permanent_deletion_unlinks_symlinks_not_their_targets() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep.md"), "keep").unwrap();
        for (name, destination) in [
            ("dir-link", target.clone()),
            ("file-link", target.join("keep.md")),
            ("dangling", temp.path().join("missing")),
        ] {
            let link = temp.path().join(name);
            symlink(destination, &link).unwrap();
            delete_with(
                &link,
                |path, metadata| {
                    assert!(metadata.file_type().is_symlink());
                    assert_eq!(volume_probe_path(path, metadata), path.parent().unwrap());
                    Ok(true)
                },
                |_| panic!("network link should be unlinked"),
            )
            .unwrap();
            assert!(fs::symlink_metadata(link).is_err());
            assert_eq!(fs::read_to_string(target.join("keep.md")).unwrap(), "keep");
        }
        let tree = temp.path().join("tree");
        fs::create_dir(&tree).unwrap();
        symlink(&target, tree.join("external")).unwrap();
        remove_permanently(&tree).unwrap();
        assert_eq!(fs::read_to_string(target.join("keep.md")).unwrap(), "keep");
    }

    #[cfg(windows)]
    #[test]
    fn windows_permanent_deletion_unlinks_directory_file_and_dangling_symlinks() {
        use std::os::windows::fs::{symlink_dir, symlink_file};
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep.md"), "keep").unwrap();
        let dir_link = temp.path().join("dir-link");
        if let Err(error) = symlink_dir(&target, &dir_link) {
            if error.raw_os_error() == Some(1314) {
                eprintln!(
                    "symlink fixture needs Windows Developer Mode or SeCreateSymbolicLinkPrivilege"
                );
                return;
            }
            panic!("could not create symlink fixture: {error}");
        }
        let file_link = temp.path().join("file-link");
        let dangling_dir = temp.path().join("dangling-dir");
        let dangling_file = temp.path().join("dangling-file");
        symlink_file(target.join("keep.md"), &file_link).unwrap();
        symlink_dir(temp.path().join("missing-dir"), &dangling_dir).unwrap();
        symlink_file(temp.path().join("missing-file"), &dangling_file).unwrap();
        for link in [dir_link, file_link, dangling_dir, dangling_file] {
            delete_with(
                &link,
                |path, metadata| {
                    assert!(metadata.file_type().is_symlink());
                    assert_eq!(volume_probe_path(path, metadata), path.parent().unwrap());
                    Ok(true)
                },
                |_| panic!("network symlink bypasses Trash"),
            )
            .unwrap();
            assert!(fs::symlink_metadata(&link).is_err());
            assert_eq!(fs::read_to_string(target.join("keep.md")).unwrap(), "keep");
        }
        let tree = temp.path().join("tree");
        fs::create_dir(&tree).unwrap();
        symlink_dir(&target, tree.join("external")).unwrap();
        remove_permanently(&tree).unwrap();
        assert_eq!(fs::read_to_string(target.join("keep.md")).unwrap(), "keep");
    }

    #[cfg(windows)]
    #[test]
    fn unc_and_mapped_drives_are_network_but_local_and_unknown_drives_are_not() {
        for path in [r"\\server\share\note.md", r"\\?\UNC\server\share\note.md"] {
            assert!(
                windows_network_volume(Path::new(path), |_| panic!("UNC needs no drive probe"))
                    .unwrap()
            );
        }
        assert!(windows_network_volume(Path::new(r"Z:\note.md"), |_| Ok(4)).unwrap());
        for drive in [2, 3, 5, 6] {
            assert!(!windows_network_volume(Path::new(r"C:\note.md"), |_| Ok(drive)).unwrap());
        }
        assert!(!windows_network_volume(Path::new(r"\\?\C:\note.md"), |_| Ok(3)).unwrap());
        for drive in [0, 1, 7, u32::MAX] {
            assert!(windows_network_volume(Path::new(r"Z:\note.md"), |_| Ok(drive)).is_err());
        }
        assert!(
            windows_network_volume(Path::new(r"Z:\note.md"), |_| Err(
                std::io::Error::from_raw_os_error(5)
            ))
            .is_err()
        );
    }
}
