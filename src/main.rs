use mp3v0fs::mp3v0fs::Mp3V0Fs;
use simplelog::{CombinedLogger, LevelFilter, Config, SimpleLogger};
use std::env;
use std::ffi::{OsStr, OsString};
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
        ::std::process::exit(-1);
    }

    let filesystem = Mp3V0Fs::new(args[1].clone());

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
