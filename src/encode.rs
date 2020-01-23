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
const MAX_VBR_FRAME_SIZE: usize = 2880;

/// The `Encode` trait allows for encoding audio data from a reader to a specific format.
///
/// Implementors of the `Encode` trait define an [`encode()`] method that describes the
/// specifics of converting a particular filetype to mp3.
pub trait Encode<R: io::Read> {

    /// Returns a chunk of encoded mp3 data of the requested size.
    /// This functions maintains state about where it is in the data stream, and returns
    /// the next chunk of encoded mp3 data on subsequent calls.
    fn read(&mut self, size: u32) -> Vec<u8> {
        if !self.get_encoding_finished() {
            while self.encode(size as usize) > 0 {
                continue
            }
            self.encode_finalize();
        }

        let output_buffer = self.get_output_buffer_mut();
        let encoded_chunk_size = min(size as usize, output_buffer.len());
        let mut encoded_chunk: Vec<u8> = Vec::with_capacity(min(size as usize, output_buffer.len()));
        for _i in 0..encoded_chunk_size {
            encoded_chunk.push(output_buffer.pop_front().unwrap());
        }

        encoded_chunk
    }

    /// Encodes the next chunk of data.
    /// Returns the length of encoded data written to the output_buffer.
    fn encode(&mut self, size: usize) -> usize;

    /// Performs the last steps of the encode, e.g. flushing buffers. Should be called once after encode has nothing
    /// left to write.
    /// Returns the length of encoded data written to the output_buffer.
    fn encode_finalize(&mut self) -> usize;

    /// Estimate the final encoded file size. This should return an upper bound in bytes.
    fn calculate_size(&mut self) -> u64;

    /// Get the output buffer used to temporarily store encoded mp3 data.
    fn get_output_buffer(&self) -> &VecDeque<u8>;
    /// Get the (mutable) output buffer used to temporarily store encoded mp3 data.
    fn get_output_buffer_mut(&mut self) -> &mut VecDeque<u8>;

    /// Whether or not encoding has been finished.
    fn get_encoding_finished(&mut self) -> bool;
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
    // Size (in bytes) of tags
    tag_size: usize,
    encoding_finished: bool,
    output_buffer: VecDeque<u8>
}

/// Encoder for a FLAC file.
impl FlacToMp3Encoder<File> {

    pub fn new(flac_reader: FlacReader<File>) -> FlacToMp3Encoder<File> {
        // 8MB
        let mut output_buffer = VecDeque::with_capacity(8388608);
        // Initialize tags
        let flac_tags = flac_reader.tags();
        let tag_size = FlacToMp3Encoder::initialize_tags(flac_tags, &mut output_buffer);

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
            tag_size,
            encoding_finished: false,
            output_buffer
        }
    }

    /// Injects tag data into the output stream, which should happen before encoding starts.
    fn initialize_tags(flac_tags: Tags, output_buffer: &mut VecDeque<u8>) -> usize {
        let mut tag_buffer: Cursor<Vec<u8>> = Cursor::new(Vec::with_capacity(2048));
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
        tag_buffer.get_ref().len()
    }
}

/// Implementation of Encoder that converts FLAC to MP3.
impl Encode<File> for FlacToMp3Encoder<File> {

    fn encode(&mut self, size: usize) -> usize {
        //TODO can this memory be recycled?
        let mut pcm_left: Vec<i16> = Vec::with_capacity(size);
        let mut pcm_right: Vec<i16> = Vec::with_capacity(size);

        for _ in 0..size*2 {
            match self.flac_samples.next() {
                // TODO support 24-bit FLAC
                Some(l_frame) => pcm_left.push(l_frame.unwrap() as i16),
                None => {
                    break;
                }
            };

            match self.flac_samples.next() {
                // TODO support 24-bit FLAC
                Some(r_frame) => pcm_right.push(r_frame.unwrap() as i16),
                None => {
                    break;
                }
            };
        }

        let sample_count = pcm_right.len();

        // Worst case buffer size estimate per LAME docs
        let mut lame_buffer = vec![0; 5*sample_count/4 + 7200];
        let mut lame = self.lame_wrapper.lame.lock().unwrap();
        let output_length = match lame.encode_buffer(
            pcm_left.as_mut_slice(), pcm_right.as_mut_slice(), &mut lame_buffer
        ) {
            Ok(output_length) => output_length,
            Err(err) => panic!("Unexpected error encoding PCM data: {:?}", err),
        };
        lame_buffer.truncate(output_length);

        for byte in lame_buffer {
            self.output_buffer.push_back(byte);
        }
        output_length
    }

    fn encode_finalize(&mut self) -> usize {
        // Collect remaining output of internal LAME buffers once we reach the end
        // of the PCM data stream
        let mut lame_buffer = vec![0; 7200];
        let mut lame = self.lame_wrapper.lame.lock().unwrap();
        let flush_output_length = match lame.encode_flush(&mut lame_buffer) {
            Ok(output_length) => output_length,
            Err(err) => panic!("Unexpected error flushing LAME buffers: {:?}", err)
        };
        lame_buffer.truncate(flush_output_length);

        for byte in lame_buffer {
            self.output_buffer.push_back(byte);
        }

        let mut vbr_buffer = vec![0; MAX_VBR_FRAME_SIZE];
        let vbr_frame_length = lame.get_vbr_tag(&mut vbr_buffer);
        vbr_buffer.truncate(vbr_frame_length);
        let mut index = 0;
        for byte in vbr_buffer {
            std::mem::replace(&mut self.output_buffer[self.tag_size + index], byte);
            index += 1;
        }
        self.encoding_finished = true;

        flush_output_length
    }

    fn calculate_size(&mut self) -> u64 {
        let sample_count = self.stream_info.samples.expect("Unable to get PCM sample count");
        let mut lame = self.lame_wrapper.lame.lock().unwrap();
        let bitrate = lame.get_vbr_max_bitrate();
        let samplerate = lame.get_out_samplerate();

        self.tag_size as u64 + MAX_VBR_FRAME_SIZE as u64
            + ((sample_count * 144 * u64::from(bitrate) * 10) / (u64::from(samplerate) / 100))
    }

    fn get_output_buffer(&self) -> &VecDeque<u8> {
        return self.output_buffer.borrow();
    }

    fn get_output_buffer_mut(&mut self) -> &mut VecDeque<u8> {
        return self.output_buffer.borrow_mut();
    }

    fn get_encoding_finished(&mut self) -> bool {
        return self.encoding_finished;
    }
}
