//#![warn(missing_docs, bad_style, unused, unused_extern_crates, unused_import_braces, unused_qualifications, missing_debug_implementations)]
extern crate libc;
#[macro_use]
extern crate log;
extern crate simplelog;
extern crate time;

pub mod encode;
pub mod libc_util;
pub mod mp3v0fs;
pub mod tags;

use mp3v0fs::Mp3V0Fs;

use std::ffi::{OsString, OsStr};
use std::io::Result;

pub fn run(target: &OsString, mountpoint: &OsString, fuse_args: &Vec<&OsStr>) -> Result<()> {
    let filesystem = Mp3V0Fs::new(target.clone());

    fuse_mt::mount(
        fuse_mt::FuseMT::new(filesystem, 1), &mountpoint, fuse_args
    )
}
