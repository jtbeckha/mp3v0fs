use mp3v0fs::run;

use simplelog::{CombinedLogger, LevelFilter, Config, SimpleLogger};
use std::env;
use std::ffi::{OsString, OsStr};
use std::process::exit;

fn main() {
    // Initialize logging
    CombinedLogger::init(
        vec![
            SimpleLogger::new(LevelFilter::Debug, Config::default()),
        ]
    ).unwrap();

    if cfg!(windows) {
        println!("windows is not supported");
        exit(1);
    }

    let args: Vec<OsString> = env::args_os().collect();

    if args.len() != 3 {
        println!("usage: {} <target> <mountpoint>", &env::args().next().unwrap());
        exit(1);
    }

    let target = args[1].clone();
    let mountpoint = args[2].clone();

    let fuse_args: Vec<&OsStr> = vec![
        &OsStr::new("-o"), &OsStr::new("auto_unmount"),
        &OsStr::new("-o"), &OsStr::new("rdonly")
    ];

    match run(&target, &mountpoint, &fuse_args) {
        Ok(()) => (),
        Err(err) => println!("Error occurred {}", err)
    }
}
