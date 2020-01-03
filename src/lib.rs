//#![warn(missing_docs, bad_style, unused, unused_extern_crates, unused_import_braces, unused_qualifications, missing_debug_implementations)]
extern crate libc;
#[macro_use]
extern crate log;
extern crate simplelog;
extern crate time;

pub mod encode;
pub mod lame;
pub mod libc_util;
pub mod mp3v0fs;
pub mod tags;
pub mod inode;

use crate::mp3v0fs::Mp3V0Fs;

use std::ffi::{OsString, OsStr};
use std::io::Result;
use fuse::BackgroundSession;

pub fn run(target: &OsString, mountpoint: &OsString, fuse_args: &Vec<&OsStr>) -> Result<()> {
    let filesystem = Mp3V0Fs::new(target.clone());

    fuse::mount(
        fuse_mt::FuseMT::new(filesystem, 1), mountpoint, fuse_args
    )
}

pub fn run_async<'a>(target: &OsString, mountpoint: &OsString, fuse_args: &Vec<&OsStr>) -> Result<BackgroundSession<'a>> {
    let filesystem = Mp3V0Fs::new(target.clone());

    unsafe {
        fuse::spawn_mount(
            fuse_mt::FuseMT::new(filesystem, 1), mountpoint, fuse_args
        )
    }
}
