use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs::{File, read_dir};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::vec::Vec;

use crate::encode::{Encode, FlacToMp3Encoder};
use claxon::FlacReader;
use std::sync::{Arc, Mutex};
use fuse::{Filesystem, FileAttr, FileType, ReplyOpen, ReplyAttr, ReplyData, ReplyXattr, ReplyEmpty, Request, ReplyEntry, ReplyDirectory};
use crate::inode::{InodeTable, Inode};
use std::time::Duration;

const FLAC: &'static str = "flac";
const MP3: &'static str = "mp3";
const TTL: Duration = Duration::from_secs(1);

pub struct Mp3V0Fs {
    pub target: OsString,
    fds: Arc<Mutex<HashMap<u64, FlacToMp3Encoder<File>>>>,
    inode_table: InodeTable
}

impl Mp3V0Fs {

    pub fn new(target: OsString) -> Mp3V0Fs {
        Mp3V0Fs {
            target,
            fds: Arc::new(Mutex::new(HashMap::new())),
            inode_table: InodeTable::new()
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

    //TODO handle mp3 passthrough
    fn fuse_path(&self, real_path: &Path) -> PathBuf {
        let partial = real_path.strip_prefix(&self.target).unwrap();

        return PathBuf::from("/")
            .join(partial);
    }

    fn stat(&self, ino: Inode, fuse_path: &PathBuf) -> Result<FileAttr, std::io::Error> {
        let real_path: OsString = self.real_path(fuse_path);
        let metadata = match std::fs::metadata(real_path) {
            Ok(metadata) => metadata,
            Err(e) => return Err(e)
        };

        let fuse_filetype = match adapt_filetype(metadata.file_type()) {
            Some(fuse_filetype) => fuse_filetype,
            //TODO error code enum
            None => return Err(std::io::Error::last_os_error())
        };

        Ok(fuse::FileAttr {
            ino,
            // TODO calculate
            size: metadata.size(),
            blocks: metadata.blocks(),
            //TODO error checking
            atime: metadata.accessed().unwrap(),
            mtime: metadata.modified().unwrap(),
            ctime: metadata.modified().unwrap(),
            crtime: metadata.modified().unwrap(),
            kind: fuse_filetype,
            perm: metadata.mode() as u16,
            nlink: metadata.nlink() as u32,
            uid: metadata.uid(),
            gid: metadata.gid(),
            rdev: metadata.rdev() as u32,
            flags: 0
        })
    }
}

impl Filesystem for Mp3V0Fs {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let (inode, path) = self.inode_table.lookup(parent, name);
        debug!("lookup: {:?}, {:?}", inode, path);
        //TODO convert .flac to .mp3

        match self.stat(inode, &path) {
            Ok(attr) => reply.entry(&self::TTL, &attr, 1),
            Err(_e) => reply.error(1)
        };
    }

    fn forget(&mut self, _req: &Request, ino: u64, nlookup: u64) {
        debug!("forget: {:?}, {:?}", ino, nlookup);
        unimplemented!()
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        let path = match self.inode_table.get_path(ino) {
            Some(path) => path,
            None => return reply.error(1)
        }.to_owned();
        debug!("getattr: {:?}", path);

        match self.stat(ino, &path) {
            Ok(attr) => reply.attr(&self::TTL, &attr),
            Err(_e) => reply.error(1)
        };
    }

    fn readlink(&mut self, _req: &Request, ino: u64, _reply: ReplyData) {
        //TODO needed?
        debug!("readlink: {:?}", ino);
        unimplemented!()
    }

    fn open(&mut self, _req: &Request, ino: u64, flags: u32, reply: ReplyOpen) {
        let path = match self.inode_table.get_path(ino) {
            Some(path) => path,
            None => return reply.error(1)
        }.to_owned();
        debug!("open: {:?}, {:?}", path, flags);

        let real_path = self.real_path(&path);
        let mut fds = self.fds.lock().unwrap();

        if !fds.contains_key(&ino) {
            let flac_reader = match FlacReader::open(real_path.to_owned()) {
                Ok(flac_reader) => flac_reader,
                Err(err) => panic!("Error opening file {}. {}", path.to_str().unwrap(), err)
            };

            let encoder = FlacToMp3Encoder::new(flac_reader);

            debug!("adding ino={} to fds", ino);
            fds.insert(ino, encoder);
        }

        // inode number is always be unique per file so should be an acceptable replacement for the
        // fh u64 expected in ReplyOpen
        reply.opened(ino, flags);
    }

    fn read(&mut self, _req: &Request, ino: u64, fh: u64, offset: i64, size: u32, reply: ReplyData) {
        let path = match self.inode_table.get_path(ino) {
            Some(path) => path,
            // TODO error code enum
            None => return reply.error(1)
        }.to_owned();
        debug!("read: {:?}, {:?}, {:?}, {:?}", fh, path, offset, size);

        let mut fds = self.fds.lock().unwrap();
        let encoder = match fds.get_mut(&fh) {
            Some(encoder) => encoder,
            None => panic!("Failed to read encoder from fds")
        };

        let data = encoder.read(size);
        reply.data(&data);
    }

    fn release(&mut self, _req: &Request, ino: u64, fh: u64, flags: u32, lock_owner: u64, flush: bool, _reply: ReplyEmpty) {
        debug!("release: {:?}, {:?}, {:?}, {:?}, {:?}", ino, fh, flags, lock_owner, flush);
        unimplemented!()
    }

    fn opendir(&mut self, _req: &Request, ino: u64, flags: u32, reply: ReplyOpen) {
        let path = match self.inode_table.get_path(ino) {
            Some(path) => path,
            // TODO error code enum
            None => return reply.error(1)
        }.to_owned();
        debug!("opendir: {:?}, {:?}", path, flags);

        // inode number is always be unique per file so should be an acceptable replacement for the
        // fh u64 expected in ReplyOpen
        reply.opened(ino, flags);
    }

    //TODO handle chunking responses
    fn readdir(&mut self, _req: &Request, ino: u64, _fh: u64, _offset: i64, mut reply: ReplyDirectory) {
        let path = match self.inode_table.get_path(ino) {
            Some(path) => path,
            // TODO error code enum
            None => {
                reply.error(1);
                return;
            }
        }.to_owned();
        debug!("readdir: {:?}", path);

        let real_path = self.real_path(&path);
        let entries = match read_dir(real_path) {
            Ok(read_dir) => read_dir,
            Err(_e) => {
                //TODO error code enum
                reply.error(1);
                return;
            }
        };

        for dir_entry_result in entries {
            if dir_entry_result.is_err() {
                debug!("error reading dir_entry: {}", dir_entry_result.err().unwrap());
                continue;
            }
            let dir_entry = dir_entry_result.unwrap();

            let fuse_path = self.fuse_path(dir_entry.path().as_path());
            let inode = match self.inode_table.get_inode(&fuse_path) {
                Some(inode) => inode,
                None => continue
            };

            let fuse_filetype = match dir_entry.file_type() {
                Ok(fs_filetype) => match adapt_filetype(fs_filetype) {
                    Some(fuse_filetype) => fuse_filetype,
                    None => continue
                },
                Err(_e) => {
                    //TODO error code enum
                    reply.error(1);
                    return;
                }
            };

            let fuse_filename = parse_name(fuse_path.as_path().to_str().unwrap());

            reply.add(inode, 0, fuse_filetype, fuse_filename);
        }

        reply.ok();
    }

    fn releasedir(&mut self, _req: &Request, ino: u64, fh: u64, flags: u32, _reply: ReplyEmpty) {
        //TODO needed?
        debug!("releasedir: {:?}, {:?}, {:?}", ino, fh, flags);
        unimplemented!()
    }

    fn getxattr(&mut self, _req: &Request, ino: u64, name: &OsStr, size: u32, _reply: ReplyXattr) {
        debug!("getxattr: {:?}, {:?}, {:?}", ino, name, size);
        unimplemented!()
    }

    fn listxattr(&mut self, _req: &Request, ino: u64, size: u32, _reply: ReplyXattr) {
        debug!("listxattr: {:?}, {:?}", ino, size);
        unimplemented!()
    }
}

fn adapt_filetype(fs_filetype: std::fs::FileType) -> Option<FileType> {
    if fs_filetype.is_file() {
        return Some(FileType::RegularFile);
    } else if fs_filetype.is_dir() {
        return Some(FileType::Directory);
    } else if fs_filetype.is_symlink() {
        return Some(FileType::Symlink);
    } else {
        return None;
    }
}

/// Parses out the name of a file given a path.
///
/// # Examples (TODO make these tests)
///
/// ```
/// assert_eq!(parse_name("/home/user/test.flac"), "test.flac");
/// assert_eq!(parse_name("test.flac"), "test.flac");
/// ```
fn parse_name(path: &str) -> String {
    let path_components: Vec<&str> = path.split("/").collect();
    if path_components.len() == 0 {
        return String::from("")
    }

    path_components[path_components.len() - 1].to_owned()
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
