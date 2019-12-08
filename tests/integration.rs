use mp3v0fs::run_async;

use std::ffi::{OsString, OsStr};
use std::fs::read_dir;
use std::io::Error;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

#[test]
fn test_run() -> Result<(), Error> {
    let target_dir_path = OsString::from(format!("{}/tests/resources", env!("CARGO_MANIFEST_DIR")));

    let mount_dir = match TempDir::new_in(format!("{}/tests", env!("CARGO_MANIFEST_DIR"))) {
        Ok(dir) => dir,
        Err(err) => panic!("Failed to create mount_dir {}", err)
    };
    let mount_dir_path = OsString::from(mount_dir.path().as_os_str());

    let fuse_args: Vec<&OsStr> = vec![
        &OsStr::new("-o"), &OsStr::new("auto_unmount"),
        &OsStr::new("-o"), &OsStr::new("rdonly")
    ];

    let fs_session = run_async(&target_dir_path, &mount_dir_path, &fuse_args);
    thread::sleep(Duration::from_millis(500));

    {
        let read_dir_result = read_dir(mount_dir.path()).unwrap();
        let entry = read_dir_result.into_iter().next().unwrap().unwrap();
        assert_eq!(entry.file_name(), "C1.mp3");
    }

    // Clean up temp directories
    drop(fs_session);
    mount_dir.close()?;

    Ok(())
}
