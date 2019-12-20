use claxon::{FlacReader, FlacSamples};
use std::fs::File;
use std::io;
use claxon::input::BufferedReader;
use std::collections::VecDeque;
use crate::tags;
use id3::{Tag, Version};
use std::io::Cursor;
use std::borrow::{BorrowMut, Borrow};
use std::cmp::min;
use std::sync::{Arc, Mutex};
use claxon::metadata::StreamInfo;
use crate::lame::Lame;

/// The `Encode` trait allows for encoding data from a reader to mp3.
///
/// Implementors of the `Encode` trait define an [`encode()`] method that describes the
/// specifics of converting a particular filetype to mp3.
pub trait Encode<R: io::Read> {

    /// Returns a chunk of encoded mp3 data of the requested size.
    /// This functions maintains state about where it is in the data stream, and returns
    /// the next chunk of encoded mp3 data on subsequent calls.
    fn read(&mut self, size: u32) -> Vec<u8> {
        while self.get_mp3_buffer().len() < size as usize {
            let encoded_length = self.encode(size as usize);
            if encoded_length == 0 {
                break;
            }
        }

        let mp3_buffer = self.get_mp3_buffer_mut();
        let encoded_mp3_chunk_size = min(size as usize, mp3_buffer.len());
        let mut encoded_mp3_chunk: Vec<u8> = Vec::with_capacity(min(size as usize, mp3_buffer.len()));
        for _i in 0..encoded_mp3_chunk_size {
            encoded_mp3_chunk.push(mp3_buffer.pop_front().unwrap());
        }

        encoded_mp3_chunk
    }

    /// Encodes the next chunk of data to mp3 v0.
    /// Returns the length of encoded data written to the mp3_buffer.
    fn encode(&mut self, size: usize) -> usize;

    /// Estimate the final encoded file size.
    fn calculate_size(&mut self) -> u64;

    /// Get the mp3_buffer used to temporarily store encoded mp3 data.
    fn get_mp3_buffer(&self) -> &VecDeque<u8>;
    /// Get the (mutable) mp3_buffer used to temporarily store encoded mp3 data.
    fn get_mp3_buffer_mut(&mut self) -> &mut VecDeque<u8>;
}

/// Wrapper for Lame so it can be marked Send/Sync for fuse-mt
struct LameWrapper {
    lame: Arc<Mutex<Lame>>
}
unsafe impl Send for LameWrapper {}
unsafe impl Sync for LameWrapper {}

pub struct FlacToMp3Encoder<R: io::Read> {
    lame_wrapper: LameWrapper,
    flac_samples: FlacSamples<BufferedReader<R>>,
    stream_info: StreamInfo,
    mp3_buffer: VecDeque<u8>
}

/// Encoder for a FLAC file.
impl FlacToMp3Encoder<File> {

    pub fn new(flac_reader: FlacReader<File>, size: usize) -> FlacToMp3Encoder<File> {
        let flac_tags = flac_reader.tags();
        let mut mp3_tag = Tag::new();

        let stream_info = flac_reader.streaminfo();

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

        let mut lame = Lame::new().expect("Failed to initialize LAME context");

        lame.set_channels(2).expect("Failed to call lame.set_channels()");
//        lame.set_kilobitrate(320).expect("Failed to call lame.set_kilobitrate()");
        lame.set_quality(0).expect("Failed to call lame.set_quality()");
        lame.set_in_sample_rate(stream_info.sample_rate).expect("Failed to call lame.set_sample_rate()");
//        lame.set_channels(stream_info.channels as u8).expect("Failed to call lame.set_channels()");
        lame.init_params().expect("Failed to call lame.init_params()");

        let encoder = FlacToMp3Encoder {
            flac_samples: flac_reader.samples_owned(),
            lame_wrapper: LameWrapper {
                lame: Arc::from(Mutex::new(lame))
            },
            stream_info,
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

    fn encode(&mut self, size: usize) -> usize {
        //TODO figure out size calculation? Probably need some kind of lazily calculated circular
        //buffer that the FS can pull from for the mp3 data
        let mut pcm_left: Vec<i32> = Vec::with_capacity(size);
        let mut pcm_right: Vec<i32> = Vec::with_capacity(size);

        let mut should_flush = false;

        for _ in 0..size*2 {
            match self.flac_samples.next() {
                Some(l_frame) => pcm_left.push(l_frame.unwrap()),
                None => {
                    should_flush = true;
                    break;
                }
            };

            match self.flac_samples.next() {
                Some(r_frame) => pcm_right.push(r_frame.unwrap()),
                None => {
                    should_flush = true;
                    break;
                }
            };
        }

        let sample_count = pcm_right.len();

        // Worst case buffer size estimate per LAME docs
        let mut lame_buffer = vec![0; 5*sample_count/4 + 7200];
        let mut lame = self.lame_wrapper.lame.lock().unwrap();
        let mut output_length = match lame.encode_buffer(
            pcm_left.as_mut_slice(), pcm_right.as_mut_slice(), &mut lame_buffer
        ) {
            Ok(output_length) => output_length,
            Err(err) => panic!("Unexpected error encoding PCM data: {:?}", err),
        };
        lame_buffer.truncate(output_length);

        for byte in lame_buffer {
            self.mp3_buffer.push_back(byte);
        }

        // Collect remaining output of internal LAME buffers once we reach the end
        // of the PCM data stream
        if should_flush {
            let mut lame_buffer = vec![0; 7200];
            let flush_output_length = match lame.encode_flush(&mut lame_buffer) {
                Ok(output_length) => output_length,
                Err(err) => panic!("Unexpected error flushing LAME buffers: {:?}", err)
            };
            lame_buffer.truncate(flush_output_length);

            for byte in lame_buffer {
                self.mp3_buffer.push_back(byte);
            }

            output_length = output_length + flush_output_length;
        }

        output_length
    }

    fn calculate_size(&mut self) -> u64 {
        unimplemented!();
    }

    fn get_mp3_buffer(&self) -> &VecDeque<u8> {
        return self.mp3_buffer.borrow();
    }

    fn get_mp3_buffer_mut(&mut self) -> &mut VecDeque<u8> {
        return self.mp3_buffer.borrow_mut();
    }
}
