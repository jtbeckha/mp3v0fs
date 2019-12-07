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
