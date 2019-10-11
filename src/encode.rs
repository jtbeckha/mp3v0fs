use lame::Lame;
use claxon::{FlacReader, FlacSamples};
use std::fs::File;
use claxon::input::{BufferedReader, ReadBytes};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub struct Encoder<R: ReadBytes> {
    flac_samples: FlacSamples<R>,
    mp3_buffer: VecDeque<u8>
}

impl<'r> Encoder<&'r mut BufferedReader<File>> {
//    pub fn new(flac_reader: &mut Arc<Mutex<FlacReader<File>>>) -> Encoder<&'r mut BufferedReader<File>> {
//        Encoder {
//            flac_samples: flac_reader.to_owned().lock().unwrap().samples(),
//            mp3_buffer: VecDeque::new()
//        }
//    }

    /// Returns a chunk of encoded mp3 v0 data of the requested size.
    /// This functions maintains state about where it is at in the FLAC stream, and will
    /// return the next chunk of encoded mp3 data on subsequent calls.
    pub fn read(&mut self, lame: &mut Lame, size: u32) -> Vec<u8> {
        while self.mp3_buffer.len() < size as usize {
            //TODO check for EOF as well
            self.encode(lame, size as usize);
        }

        let mut encoded_mp3_chunk: Vec<u8> = vec![0; size as usize];
        for _i in 0..size {
            encoded_mp3_chunk.push(self.mp3_buffer.pop_front().unwrap());
        }

        return encoded_mp3_chunk.to_owned();
    }

    fn encode(&mut self, lame: &mut Lame, size: usize) {
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
        }

        let mut lame_buffer = vec![0; size];
        lame.encode(pcm_left.as_slice(), pcm_right.as_slice(), &mut lame_buffer);
        for byte in lame_buffer {
            self.mp3_buffer.push_back(byte);
        }
    }
}
