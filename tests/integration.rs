use mp3v0fs::run_async;

use std::ffi::{OsString, OsStr};
use std::fs::{read_dir, File};
use std::io::Error;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

#[test]
fn test_filesystem() -> Result<(), Error> {
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
    thread::sleep(Duration::from_millis(50));

    {
        let read_dir_result = read_dir(mount_dir.path())?;
        let entry = read_dir_result.into_iter().next().unwrap()?;
        assert_eq!("C1.mp3", entry.file_name());

        // Decoding the resulting mp3 and counting the frames should give us some confidence
        // in the integrity of the file
        let mp3_file = File::open(entry.path())?;
        let mut decoder = simplemad::Decoder::decode(mp3_file).unwrap();
        let mut frame_count = 0;

        // For some reason simplemad returns an error on the first frame even for mp3s encoded
        // directly with ffmpeg, so skip past it.
        let _error_frame = decoder.get_frame();
        for frame_result in decoder {
            frame_result.unwrap();
            frame_count += 1;
        }

        assert_eq!(19, frame_count);
    }

    // Drop the mounted fs and ensure the temporary mountpoint is cleaned up
    drop(fs_session);
    mount_dir.close()?;

    Ok(())
}
