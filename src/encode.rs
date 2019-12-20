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
use claxon::metadata::{StreamInfo, Tags};
use crate::lame::Lame;
use lame_sys::vbr_mode::vbr_mtrh;

// From LAME
const MAX_VBR_FRAME_SIZE: u64 = 2880;

/// The `Encode` trait allows for encoding audio data from a reader to a specific format.
///
/// Implementors of the `Encode` trait define an [`encode()`] method that describes the
/// specifics of converting a particular filetype to mp3.
pub trait Encode<R: io::Read> {

    /// Returns a chunk of encoded mp3 data of the requested size.
    /// This functions maintains state about where it is in the data stream, and returns
    /// the next chunk of encoded mp3 data on subsequent calls.
    fn read(&mut self, size: u32) -> Vec<u8> {
        // Lazily set buffer capacity, since we don't know the chunk size that will be requested
        // until read is called for the first time.
        if self.get_output_buffer().capacity() == 0 {
            self.get_output_buffer_mut().reserve((size * 2) as usize);
        }

        while self.get_output_buffer().len() < size as usize {
            let encoded_length = self.encode(size as usize);
            if encoded_length == 0 {
                break;
            }
        }

        let output_buffer = self.get_output_buffer_mut();
        let encoded_chunk_size = min(size as usize, output_buffer.len());
        let mut encoded_chunk: Vec<u8> = Vec::with_capacity(min(size as usize, output_buffer.len()));
        for _i in 0..encoded_chunk_size {
            encoded_chunk.push(output_buffer.pop_front().unwrap());
        }

        encoded_chunk
    }

    /// Encodes the next chunk of data to mp3 v0.
    /// Returns the length of encoded data written to the mp3_buffer.
    fn encode(&mut self, size: usize) -> usize;

    /// Estimate the final encoded file size. This should return an upper bound in bytes.
    fn calculate_size(&mut self) -> u64;

    /// Get the output buffer used to temporarily store encoded mp3 data.
    fn get_output_buffer(&self) -> &VecDeque<u8>;
    /// Get the (mutable) output buffer used to temporarily store encoded mp3 data.
    fn get_output_buffer_mut(&mut self) -> &mut VecDeque<u8>;
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
    tag_buffer: Cursor<Vec<u8>>,
    output_buffer: VecDeque<u8>
}

/// Encoder for a FLAC file.
impl FlacToMp3Encoder<File> {

    pub fn new(flac_reader: FlacReader<File>) -> FlacToMp3Encoder<File> {
        // Initialize tags
        let flac_tags = flac_reader.tags();
        let mut tag_buffer = Cursor::new(Vec::with_capacity(2048));
        let mut output_buffer = VecDeque::with_capacity(2048);
        FlacToMp3Encoder::initialize_tags(flac_tags, &mut tag_buffer, &mut output_buffer);

        let stream_info = flac_reader.streaminfo();
        // Initialize LAME
        let mut lame = Lame::new().expect("Failed to initialize LAME context");
        lame.set_channels(stream_info.channels).expect("Failed to call lame.set_channels()");
        lame.set_in_samplerate(stream_info.sample_rate).expect("Failed to call lame.set_in_samplerate()");
        lame.set_vbr(vbr_mtrh).expect("Failed to call lame.set_vbr()");
        lame.set_vbr_quality(0).expect("Failed to call lame.set_vbr_quality()");
        lame.set_vbr_max_bitrate(320).expect("Failed to call lame.set_vbr_max_bitrate()");
        lame.set_write_vbr_tag(true).expect("Failed to call lame.set_write_vbr_tag()");
        lame.init_params().expect("Failed to call lame.init_params()");

        FlacToMp3Encoder {
            flac_samples: flac_reader.samples_owned(),
            lame_wrapper: LameWrapper {
                lame: Arc::from(Mutex::new(lame))
            },
            stream_info,
            tag_buffer,
            output_buffer
        }
    }

    /// Injects tag data into the output stream, which should happen before encoding starts.
    fn initialize_tags(flac_tags: Tags, tag_buffer: &mut Cursor<Vec<u8>>, output_buffer: &mut VecDeque<u8>) {
        let mut mp3_tag = Tag::new();

        for tag in flac_tags {
            match tags::translate_vorbis_comment_to_id3(
                &String::from(tag.0), &String::from(tag.1)
            ) {
                Some(frame) => mp3_tag.add_frame(frame),
                None => None
            };
        }

        mp3_tag.write_to(tag_buffer.borrow_mut(), Version::Id3v23).expect("Failed to write tags");

        for byte in tag_buffer.get_ref() {
            output_buffer.push_back(byte.clone());
        }
    }

}

/// Implementation of Encoder that converts FLAC to MP3.
impl Encode<File> for FlacToMp3Encoder<File> {

    fn encode(&mut self, size: usize) -> usize {
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
            self.output_buffer.push_back(byte);
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
                self.output_buffer.push_back(byte);
            }

            output_length = output_length + flush_output_length;
        }

        output_length
    }

    fn calculate_size(&mut self) -> u64 {
        let tag_size = self.tag_buffer.get_ref().len();
        let sample_count = self.stream_info.samples.expect("Unable to get PCM sample count");
        let mut lame = self.lame_wrapper.lame.lock().unwrap();
        let bitrate = lame.get_vbr_max_bitrate();
        let samplerate = lame.get_vbr_max_bitrate();

        tag_size as u64 + MAX_VBR_FRAME_SIZE
            + ((sample_count * 144 * u64::from(bitrate) * 10) / (u64::from(samplerate) / 100))
    }

    fn get_output_buffer(&self) -> &VecDeque<u8> {
        return self.output_buffer.borrow();
    }

    fn get_output_buffer_mut(&mut self) -> &mut VecDeque<u8> {
        return self.output_buffer.borrow_mut();
    }
}
