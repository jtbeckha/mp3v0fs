use std::collections::HashMap;
use std::ffi::{CStr, OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::vec::Vec;
use time::Timespec;

use super::libc_util::libc_extras::libc;
use super::libc_util::libc_wrappers;

use fuse_mt::*;
use crate::encode::{Encode, FlacToMp3Encoder};
use claxon::FlacReader;
use std::sync::{Arc, Mutex};
use lame::Lame;

const FLAC: &'static str = "flac";
const MP3: &'static str = "mp3";
const TTL: Timespec = Timespec { sec: 1, nsec: 0 };

/// Set of FileTypes this FS is concerned with. Everything else will be filtered out of
/// directory listings.
const RELEVANT_FILETYPES: [&'static FileType; 3] = [
    &FileType::Directory, &FileType::RegularFile, &FileType::Symlink
];

pub struct Mp3V0Fs {
    pub target: OsString,
    lame_wrapper: LameWrapper,
    fds: Arc<Mutex<HashMap<u64, FlacToMp3Encoder<File>>>>
}

/// Wrapper to allow Lame to be shared across threads (which should be safe according to
/// this thread, since we are only using the encoder:
/// https://sourceforge.net/p/lame/mailman/lame-dev/thread/01b001c40cd8%2408e80870%240c01a8c0%40Stevo03/)
struct LameWrapper {
    lame: Arc<Mutex<Lame>>
}
unsafe impl Send for LameWrapper {}
unsafe impl Sync for LameWrapper {}

impl Mp3V0Fs {

    pub fn new(target: OsString) -> Mp3V0Fs {
        let mut lame = match Lame::new() {
            Some(lame) => lame,
            None => panic!("Failed to initialize LAME MP3 encoder")
        };

        lame.set_channels(2).expect("Failed to call lame.set_channels()");
        lame.set_quality(0).expect("Failed to call lame.set_quality()");
        lame.init_params().expect("Failed to call lame.init_params()");

        Mp3V0Fs {
            target,
            lame_wrapper: LameWrapper { lame: Arc::new(Mutex::new(lame)) },
            fds: Arc::new(Mutex::new(HashMap::new()))
        }
    }

    fn real_path(&self, partial: &Path) -> OsString {
        let partial = partial.strip_prefix("/").unwrap();
        let original_candidate = PathBuf::from(&self.target)
            .join(partial);

        if original_candidate.exists() {
            return original_candidate.into_os_string();
        }

        // If the original candidate didn't exist, assume a FLAC alias does
        let flac_partial = replace_extension(partial.to_str().unwrap(), FLAC);
        return PathBuf::from(&self.target)
            .join(flac_partial)
            .into_os_string();
    }

    fn stat_real(&self, path: &Path) -> io::Result<FileAttr> {
        let real: OsString = self.real_path(path);
        debug!("stat_real: {:?}", real);

        match libc_wrappers::lstat(real) {
            Ok(stat) => {
                Ok(stat_to_fuse(stat))
            },
            Err(e) => {
                let err = io::Error::from_raw_os_error(e);
                error!("lstat({:?}): {}", path, err);
                Err(err)
            }
        }
    }
}

impl FilesystemMT for Mp3V0Fs {

    fn init(&self, _req: RequestInfo) -> ResultEmpty {
        debug!("init");
        Ok(())
    }

    fn destroy(&self, _req: RequestInfo) {
        debug!("destroy");
    }

    fn getattr(&self, _req: RequestInfo, path: &Path, fh: Option<u64>) -> ResultEntry {
        debug!("getattr: {:?}", path);

        if let Some(fh) = fh {
            match libc_wrappers::fstat(fh) {
                Ok(stat) => Ok((TTL, stat_to_fuse(stat))),
                Err(e) => Err(e)
            }
        } else {
            match self.stat_real(path) {
                Ok(attr) => Ok((TTL, attr)),
                Err(e) => Err(e.raw_os_error().unwrap())
            }
        }
    }

    fn readlink(&self, _req: RequestInfo, path: &Path) -> ResultData {
        debug!("readlink: {:?}", path);

        let real = self.real_path(path);
        match ::std::fs::read_link(real) {
            Ok(target) => Ok(target.into_os_string().into_vec()),
            Err(e) => Err(e.raw_os_error().unwrap()),
        }
    }

    fn open(&self, _req: RequestInfo, path: &Path, flags: u32) -> ResultOpen {
        debug!("open: {:?} flags={:#x}", path, flags);

        let real = self.real_path(path);
        match libc_wrappers::open(real, flags as libc::c_int) {
            Ok(fh) => Ok((fh, flags)),
            Err(e) => {
                error!("open({:?}): {}", path, io::Error::from_raw_os_error(e));
                Err(e)
            }
        }
    }

    fn read(&self, _req: RequestInfo, path: &Path, fh: u64, offset: u64, size: u32, result: impl FnOnce(Result<&[u8], libc::c_int>)) {
        debug!{"read: {:?} offset {:?}", path, offset};

        let path = self.real_path(path);

        let mut fds = self.fds.lock().unwrap();

        // TODO move this to open(), also block calls to open a file that is already open
        if !fds.contains_key(&fh) {
            let flac_reader = match FlacReader::open(path.to_owned()) {
                Ok(flac_reader) => flac_reader,
                Err(err) => panic!("Error opening file {}. {}", path.to_str().unwrap(), err)
            };

            let encoder = FlacToMp3Encoder::new(flac_reader, size as usize);

            debug!("adding fh={} to fds", fh);
            fds.insert(fh, encoder);
        }

        let encoder = match fds.get_mut(&fh) {
            Some(encoder) => encoder,
            None => panic!("Failed to read encoder from fds")
        };

        let mut lame = self.lame_wrapper.lame.lock().unwrap();

        let data = encoder.read(&mut lame, size);

        //TODO drop the encoder once we reach EOF or if some error occurs

        result(Ok(&data))
    }

    fn release(&self, _req: RequestInfo, path: &Path, fh: u64, _flags: u32, _lock_owner: u64, _flush: bool) -> ResultEmpty {
        debug!("release: {:?}", path);

        let mut fds = self.fds.lock().unwrap();
        if fds.contains_key(&fh) {
            debug!("removing fh={} from fds", fh);
            fds.remove(&fh);
        }

        Ok(())
    }

    fn opendir(&self, _req: RequestInfo, path: &Path, flags: u32) -> ResultOpen {
        let real = self.real_path(path);
        debug!("opendir: {:?} (flags = {:#o})", real, flags);
        match libc_wrappers::opendir(real) {
            Ok(fh) => Ok((fh, 0)),
            Err(e) => {
                let ioerr = io::Error::from_raw_os_error(e);
                error!("opendir({:?}): {}", path, ioerr);
                Err(e)
            }
        }
    }

    fn readdir(&self, _req: RequestInfo, path: &Path, fh: u64) -> ResultReaddir {
        debug!("readdir: {:?}", path);
        let mut entries: Vec<DirectoryEntry> = vec![];

        if fh == 0 {
            error!("readdir: missing fh");
            return Err(libc::EINVAL);
        }

        loop {
            match libc_wrappers::readdir(fh) {
                Ok(Some(entry)) => {
                    let name_c = unsafe { CStr::from_ptr(entry.d_name.as_ptr()) };
                    let name = OsStr::from_bytes(name_c.to_bytes()).to_owned();
                    let entry_path = PathBuf::from(path).join(&name);
                    let real_path = self.real_path(&entry_path);

                    let filetype = match entry.d_type {
                        libc::DT_DIR => FileType::Directory,
                        libc::DT_REG => FileType::RegularFile,
                        libc::DT_LNK => FileType::Symlink,
                        libc::DT_BLK => FileType::BlockDevice,
                        libc::DT_CHR => FileType::CharDevice,
                        libc::DT_FIFO => FileType::NamedPipe,
                        libc::DT_SOCK => {
                            warn!("FUSE doesn't support Socket file type; translating to NamedPipe instead.");
                            FileType::NamedPipe
                        },
                        _ => {
                            match libc_wrappers::lstat(real_path.clone()) {
                                Ok(stat64) => mode_to_filetype(stat64.st_mode),
                                Err(errno) => {
                                    let ioerr = io::Error::from_raw_os_error(errno);
                                    panic!("lstat failed after readdir_r gave no file type for {:?}: {}",
                                           entry_path, ioerr);
                                }
                            }
                        }
                    };

                    if !RELEVANT_FILETYPES.contains(&&filetype) {
                        continue;
                    }

                    let fuse_file_name: OsString = match filetype {
                        FileType::RegularFile | FileType::Symlink => {
                            let file_extension = parse_extension(real_path.to_str().unwrap());
                            match file_extension.as_ref() {
                                FLAC => OsString::from(replace_extension(name.to_str().unwrap(), MP3)),
                                // TODO implement passthrough reads for pre-existing MP3s
                                // MP3 => name,
                                // Filter out any filetypes we don't care about
                                _ => continue
                            }
                        },
                        _ => name
                    };

                    entries.push(DirectoryEntry {
                        name: fuse_file_name,
                        kind: filetype
                    });
                },
                Ok(None) => { break; },
                Err(e) => {
                    error!("readdir: {:?}: {}", path, e);
                    return Err(e);
                }
            }
        }

        Ok(entries)
    }

    fn getxattr(&self, _req: RequestInfo, path: &Path, name: &OsStr, size: u32) -> ResultXattr {
        debug!("getxattr: {:?} {:?} {}", path, name, size);

        let real = self.real_path(path);

        if size > 0 {
            let mut data = Vec::<u8>::with_capacity(size as usize);
            unsafe { data.set_len(size as usize) };
            let nread = libc_wrappers::lgetxattr(real, name.to_owned(), data.as_mut_slice())?;
            data.truncate(nread);
            Ok(Xattr::Data(data))
        } else {
            let nbytes = libc_wrappers::lgetxattr(real, name.to_owned(), &mut [])?;
            Ok(Xattr::Size(nbytes as u32))
        }
    }

    fn listxattr(&self, _req: RequestInfo, path: &Path, size: u32) -> ResultXattr {
        debug!("listxattr: {:?}", path);

        let real = self.real_path(path);

        if size > 0 {
            let mut data = Vec::<u8>::with_capacity(size as usize);
            unsafe { data.set_len(size as usize) };
            let nread = libc_wrappers::llistxattr(real, data.as_mut_slice())?;
            data.truncate(nread);
            Ok(Xattr::Data(data))
        } else {
            let nbytes = libc_wrappers::llistxattr(real, &mut[])?;
            Ok(Xattr::Size(nbytes as u32))
        }
    }
}

fn mode_to_filetype(mode: libc::mode_t) -> FileType {
    match mode & libc::S_IFMT {
        libc::S_IFDIR => FileType::Directory,
        libc::S_IFREG => FileType::RegularFile,
        libc::S_IFLNK => FileType::Symlink,
        libc::S_IFBLK => FileType::BlockDevice,
        libc::S_IFCHR => FileType::CharDevice,
        libc::S_IFIFO  => FileType::NamedPipe,
        libc::S_IFSOCK => FileType::Socket,
        _ => { panic!("unknown file type"); }
    }
}

fn stat_to_fuse(stat: libc::stat64) -> FileAttr {
    // st_mode encodes both the kind and the permissions
    let kind = mode_to_filetype(stat.st_mode);
    let perm = (stat.st_mode & 0o7777) as u16;

    FileAttr {
        //TODO estimate size
        size: stat.st_size as u64 * 2,
        blocks: stat.st_blocks as u64,
        atime: Timespec { sec: stat.st_atime as i64, nsec: stat.st_atime_nsec as i32 },
        mtime: Timespec { sec: stat.st_mtime as i64, nsec: stat.st_mtime_nsec as i32 },
        ctime: Timespec { sec: stat.st_ctime as i64, nsec: stat.st_ctime_nsec as i32 },
        crtime: Timespec { sec: 0, nsec: 0 },
        kind,
        perm,
        nlink: stat.st_nlink as u32,
        uid: stat.st_uid,
        gid: stat.st_gid,
        rdev: stat.st_rdev as u32,
        flags: 0,
    }
}

/// Parse out the file extension given the path to a file.
fn parse_extension(path: &str) -> String {
    let path_components: Vec<&str> = path.split("/").collect();
    if path_components.len() == 0 {
        return String::from("")
    }
    let file_name = path_components[path_components.len() - 1];

    let name_and_extension: Vec<&str> = file_name.split(".").collect();
    match name_and_extension.len() {
        0 | 1 => String::from(""),
        _ => String::from(name_and_extension[name_and_extension.len() - 1])
    }
}

/// Replaces the extension of a file with the provided replacement
fn replace_extension(path: &str, replacement: &str) -> String {
    let mut path_components: Vec<&str> = path.split("/").collect();
    if path_components.len() == 0 {
        return String::from("")
    }
    let file_name = path_components[path_components.len() - 1];

    let mut name_and_extension: Vec<&str> = file_name.split(".").collect();
    let new_filename = match name_and_extension.len() {
        0 => String::from(""),
        1 => String::from(name_and_extension[0]),
        _ => {
            name_and_extension.remove(name_and_extension.len() - 1);
            name_and_extension.push(replacement);
            name_and_extension.join(".")
        }
    };

    path_components.remove(path_components.len() - 1);
    path_components.push(&new_filename);
    path_components.join("/")
}

#[cfg(test)]
mod tests {
    use crate::mp3v0fs::{MP3, parse_extension, replace_extension};

    #[test]
    fn test_parse_extension() {
        assert_eq!("", parse_extension(""));
        assert_eq!("", parse_extension("test"));
        assert_eq!("", parse_extension("./test"));
        assert_eq!("", parse_extension("music/test"));
        assert_eq!("", parse_extension("/home/user/music/test"));
        assert_eq!("flac", parse_extension("test.flac"));
        assert_eq!("mp3", parse_extension("test.mp3"));
        assert_eq!("flac", parse_extension("./test.flac"));
        assert_eq!("mp3", parse_extension("./test.mp3"));
        assert_eq!("flac", parse_extension("music/test.flac"));
        assert_eq!("mp3", parse_extension("music/test.mp3"));
        assert_eq!("flac", parse_extension("/home/user/music/test.flac"));
        assert_eq!("mp3", parse_extension("/home/user/music/test.mp3"));
    }

    #[test]
    fn test_replace_extension() {
        assert_eq!("", replace_extension("", MP3));
        assert_eq!("test", replace_extension("test", MP3));
        assert_eq!("./test", replace_extension("./test", MP3));
        assert_eq!("/home/user/music/test", replace_extension("/home/user/music/test", MP3));
        assert_eq!("test.mp3", replace_extension("test.flac", MP3));
        assert_eq!("test.mp3", replace_extension("test.mp3", MP3));
        assert_eq!("./test.mp3", replace_extension("./test.flac", MP3));
        assert_eq!("./test.mp3", replace_extension("./test.mp3", MP3));
        assert_eq!("music/test.mp3", replace_extension("music/test.flac", MP3));
        assert_eq!("music/test.mp3", replace_extension("music/test.mp3", MP3));
        assert_eq!("/home/user/music/test.mp3", replace_extension("/home/user/music/test.flac", MP3));
        assert_eq!("/home/user/music/test.mp3", replace_extension("/home/user/music/test.mp3", MP3));
    }
}
