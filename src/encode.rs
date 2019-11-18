use lame::Lame;
use claxon::{FlacReader, FlacSamples};
use std::fs::File;
use std::io;
use claxon::input::BufferedReader;
use std::collections::VecDeque;
use crate::tags;
use id3::{Tag, Version};
use std::io::Cursor;
use std::borrow::{BorrowMut, Borrow};

/// The `Encode` trait allows for encoding data from a reader to mp3.
///
/// Implementors of the `Encode` trait define an [`encode()`] method that describes the
/// specifics of converting a particular filetype to mp3.
pub trait Encode<R: io::Read> {

    /// Returns a chunk of encoded mp3 data of the requested size.
    /// This functions maintains state about where it is at in the data stream, and will
    /// return the next chunk of encoded mp3 data on subsequent calls.
    fn read(&mut self, lame: &mut Lame, size: u32) -> Vec<u8> {
        while self.get_mp3_buffer().len() < size as usize {
            let encoded_length = self.encode(lame, size as usize);
            if encoded_length == 0 {
                break;
            }
        }

        let mp3_buffer = self.get_mp3_buffer_mut();
        let mut encoded_mp3_chunk: Vec<u8> = Vec::with_capacity(size as usize);
        for _i in 0..size {
            if !mp3_buffer.is_empty() {
                encoded_mp3_chunk.push(mp3_buffer.pop_front().unwrap());
            }
        }

        encoded_mp3_chunk
    }

    /// Encodes the next chunk of data to mp3 v0.
    /// Returns the length of encoded data written to the mp3_buffer.
    fn encode(&mut self, lame: &mut Lame, size: usize) -> usize;

    /// Get the mp3_buffer used to temporarily store encoded mp3 data.
    fn get_mp3_buffer(&self) -> &VecDeque<u8>;
    /// Get the (mutable) mp3_buffer used to temporarily store encoded mp3 data.
    fn get_mp3_buffer_mut(&mut self) -> &mut VecDeque<u8>;
}

pub struct FlacToMp3Encoder<R: io::Read> {
    flac_samples: FlacSamples<BufferedReader<R>>,
    mp3_buffer: VecDeque<u8>
}

/// Encoder for a FLAC file.
impl FlacToMp3Encoder<File> {

    pub fn new(flac_reader: FlacReader<File>, size: usize) -> FlacToMp3Encoder<File> {
        let flac_tags = flac_reader.tags();
        let mut mp3_tag = Tag::new();
        //TODO collect FLAC tags instead and store as struct member, move mp3 translation
        //and stream injection logic into init function
        for tag in flac_tags {
            match tags::translate_vorbis_comment_to_id3(
                &String::from(tag.0), &String::from(tag.1)
            ) {
                Some(frame) => mp3_tag.add_frame(frame),
                None => None
            };
        }

        let mut tag_buffer: Cursor<Vec<u8>> = Cursor::new(Vec::with_capacity(2048));
        match mp3_tag.write_to(&mut tag_buffer, Version::Id3v23) {
            Ok(()) => (),
            Err(e) => error!("Error writing tags, description={}", e.description)
        }

        let mut mp3_buffer: VecDeque<u8> = VecDeque::with_capacity(size * 2);
        for byte in tag_buffer.get_ref() {
            mp3_buffer.push_back(byte.clone());
        }

        let encoder = FlacToMp3Encoder {
            flac_samples: flac_reader.samples_owned(),
            mp3_buffer
        };
        encoder
    }

//    /// Handles work that needs to be done before PCM data starts being encoded,
//    /// e.g. injecting tag data into the stream.
//    fn initialize(&self, mp3_tag: Tag) {
//        mp3_tag.write_to(self.mp3_buffer.clone(), Version::Id3v23);
//    }

}

/// Implementation of Encoder that converts FLAC to MP3.
impl Encode<File> for FlacToMp3Encoder<File> {

    fn encode(&mut self, lame: &mut Lame, size: usize) -> usize {
        //TODO figure out size calculation? Probably need some kind of lazily calculated circular
        //buffer that the FS can pull from for the mp3 data
        let mut pcm_left: Vec<i16> = Vec::with_capacity(size);
        let mut pcm_right: Vec<i16> = Vec::with_capacity(size);

        for _i in 0..size*2 {
            let l_frame = match self.flac_samples.next() {
                Some(l_frame) => l_frame.unwrap(),
                None => break
            };
            // The FLAC decoder returns sampled in a signed 32-bit format. If we ignore 24-bit FLACs
            // for now, we can safely just convert that to an i16
            // TODO support 24-bit FLAC
            let scaled_l_frame = l_frame as i16;
            pcm_left.push(scaled_l_frame);

            let r_frame = match self.flac_samples.next() {
                Some(r_frame) => r_frame.unwrap(),
                None => break
            };
            // The FLAC decoder returns sampled in a signed 32-bit format. If we ignore 24-bit FLACs
            // for now, we can safely just convert that to an i16
            // TODO support 24-bit FLAC
            let scaled_r_frame = r_frame as i16;
            pcm_right.push(scaled_r_frame);
        }

        let sample_count = pcm_right.len();

        // I have no idea what this size calculation is, shamelessly copied from mp3fs. May or
        // not may reasonable for v0 encodings TODO learn about this
        let mut lame_buffer = vec![0; 5*sample_count/4 + 7200];
        let output_length = match lame.encode(
            pcm_left.as_slice(), pcm_right.as_slice(), &mut lame_buffer
        ) {
            Ok(output_length) => output_length,
            Err(err) => panic!("Unexpected error encoding PCM data: {:?}", err),
        };
        lame_buffer.truncate(output_length);

        for byte in lame_buffer {
            self.mp3_buffer.push_back(byte);
        }

        output_length
    }

    fn get_mp3_buffer(&self) -> &VecDeque<u8> {
        return self.mp3_buffer.borrow();
    }

    fn get_mp3_buffer_mut(&mut self) -> &mut VecDeque<u8> {
        return self.mp3_buffer.borrow_mut();
    }
}
