extern crate libc;
#[macro_use]
extern crate log;
extern crate simplelog;
extern crate time;

use fuse_mt::{FilesystemMT, RequestInfo, ResultData, ResultEmpty, ResultEntry, ResultOpen, ResultReaddir, ResultXattr};
use simplelog::{CombinedLogger, LevelFilter, Config, SimpleLogger};
use std::collections::HashMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::error::Error;
use std::io::{Seek, SeekFrom, Read};

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

    let filesystem = mp3v0fs::Mp3V0Fs {
        target: args[1].clone(),
        fds: HashMap::new()
    };

    let fuse_args: Vec<&OsStr> = vec![
        &OsStr::new("-o"), &OsStr::new("auto_unmount"),
        &OsStr::new("-o"), &OsStr::new("rdonly")
    ];

    fuse_mt::mount(fuse_mt::FuseMT::new(filesystem, 1), &args[2], &fuse_args).unwrap();
}

