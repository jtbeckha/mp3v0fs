use lame_sys::lame_encode_buffer;
use lame::Lame;
use std::borrow::{BorrowMut, Borrow};
use claxon::{FlacReader, FlacSamples};
use std::fs::File;
use claxon::input::{BufferedReader, ReadBytes};
use std::collections::VecDeque;

pub struct Encoder<R: ReadBytes> {
    lame: Lame,
//    flac_reader: FlacReader<File>,
    flac_samples: Box<FlacSamples<R>>,
    mp3_buffer: VecDeque<u8>
}

impl<R: ReadBytes> Encoder<R> {
    //TODO I guess this should return an Option? That seems to be the idiomatic approach
    pub fn new<'r>(mut flac_reader: FlacReader<File>) -> Encoder<&'r mut BufferedReader<File>> {
        let mut lame = match Lame::new() {
            Some(lame) => lame,
            None => panic!("Failed to initialize LAME MP3 encoder")
        };

        lame.set_channels(2);
        lame.set_quality(0);
        lame.init_params();

//        let lame = &mut Lame::new();
//        match lame {
//            Some(lame) => lame,
//            _ =>
//        }
//        if let Some(lame) = &mut Lame::new() {
//        lame.set_channels(2);
//        lame.set_quality(0);
//        lame.init_params();
//        } else {
//        }

//        let mut flac_reader: FlacReader<File> = match claxon::FlacReader::open("test.flac") {
//            Ok(flac_reader) => flac_reader,
//            _ => panic!("Failed to initialize Claxon FLAC decoder")
//        };

//        let flac_samples = flac_reader.samples();
//        Box::from(flac_samples);

        return Encoder {
            lame,
            flac_samples: Box::from(flac_reader.samples()),
//            flac_samples: flac_reader.samples(),
            mp3_buffer: VecDeque::with_capacity(255)
        }
    }

    /// Returns a chunk of encoded mp3 v0 data of the requested size.
    /// This functions maintains state about where it is at in the FLAC stream, and will
    /// return the next chunk of encoded mp3 data on subsequent calls.
    pub fn read(mut self, size: u32) -> Vec<u8> {
        while self.mp3_buffer.len() < size as usize {
            //TODO check for EOF as well
            self.encode(size as usize);
        }

        let mut encoded_mp3_chunk: Vec<u8> = vec![0; size as usize];
        for i in 0..size {
            encoded_mp3_chunk.push(self.mp3_buffer.pop_front().unwrap());
        }

        return encoded_mp3_chunk.to_owned();
    }

    fn encode(&mut self, size: usize) {
        //TODO figure out size calculation? Probably need some kind of lazily calculated circular
        //buffer that the FS can pull from for the mp3 data
        let mut pcm_left: Vec<i16> = vec![0; size];
        let mut pcm_right: Vec<i16> = vec![0; size];

        for i in 0..size*2 {
            if let Some(l_frame) = self.flac_samples.next() {
                let l_frame = l_frame.unwrap();
                //FIXME what to do about lossy conversion i32 -> i16?
                pcm_left.push(l_frame as i16);
            } else {
                panic!("Error reading FLAC");
            }

            if let Some(r_frame) = self.flac_samples.next() {
                let r_frame = r_frame.unwrap();
                //FIXME what to do about lossy conversion i32 -> i16?
                pcm_right.push(r_frame as i16);
            } else {
                panic!("Error reading FLAC");
            }
//            let l_frame = match self.flac_samples.next() {
//                Ok(l_frame) => l_frame,
//                None() => panic!("Should we panic here?"),
////                Err(E) => panic!("Error reading FLAC"),
//                _ => panic!("Error reading FLAC")
//            };
//            let r_frame = self.flac_samples.next();
        }

        let mut lame_buffer = vec![0; size];
        self.lame.encode(pcm_left.as_slice(), pcm_right.as_slice(), &mut lame_buffer);
        for byte in lame_buffer {
            self.mp3_buffer.push_back(byte);
        }
    }
}
