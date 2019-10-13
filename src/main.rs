//#![warn(missing_docs, bad_style, unused, unused_extern_crates, unused_import_braces, unused_qualifications, missing_debug_implementations)]
extern crate libc;
#[macro_use]
extern crate log;
extern crate simplelog;
extern crate time;

use fuse_mt::{FilesystemMT};
use simplelog::{CombinedLogger, LevelFilter, Config, SimpleLogger};
use std::env;
use std::ffi::{OsStr, OsString};

mod encode;
mod libc_extras;
mod libc_wrappers;
mod mp3v0fs;

fn main() {
    // Initialize logging
    CombinedLogger::init(
        vec![
            SimpleLogger::new(LevelFilter::Debug, Config::default()),
        ]
    ).unwrap();

    //TODO restrict to only unix systems
    let args: Vec<OsString> = env::args_os().collect();

    if args.len() != 3 {
        println!("usage: {} <target> <mountpoint>", &env::args().next().unwrap());
        ::std::process::exit(-1);
    }

    let filesystem = mp3v0fs::Mp3V0Fs::new(args[1].clone());

    let fuse_args: Vec<&OsStr> = vec![
        &OsStr::new("-o"), &OsStr::new("auto_unmount"),
        &OsStr::new("-o"), &OsStr::new("rdonly")
    ];

    match fuse_mt::mount(
        fuse_mt::FuseMT::new(filesystem, 1), &args[2], &fuse_args
    ) {
        Ok(fs) => fs,
        Err(err) => println!("Error occurred {}", err)
    }
}
