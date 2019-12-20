use lame_sys::lame_global_flags;
use std::ptr;
use std::os::raw::c_int;

pub struct Lame {
    context: *mut lame_global_flags
}

impl Lame {
    pub fn new() -> Result<Lame, Error> {
        let context = unsafe {
            lame_sys::lame_init()
        };

        if context == ptr::null_mut() {
            return Err(Error::InitializationFailed)
        } else {
            return Result::Ok(Lame {
                context
            });
        }
    }

    pub fn set_channels(&mut self, channels: u32) -> Result<(), Error> {
        handle_return_code( unsafe {
            lame_sys::lame_set_num_channels(self.context, channels as c_int)
        })
    }

    pub fn set_quality(&mut self, quality: u32) -> Result<(), Error> {
        handle_return_code(unsafe {
            lame_sys::lame_set_quality(self.context, quality as c_int)
        })
    }

    pub fn set_in_sample_rate(&mut self, sample_rate: u32) -> Result<(), Error> {
        handle_return_code(unsafe {
            lame_sys::lame_set_in_samplerate(self.context, sample_rate as c_int)
        })
    }

    pub fn init_params(&mut self) -> Result<(), Error> {
        handle_return_code(unsafe {
            lame_sys::lame_init_params(self.context)
        })
    }

    pub fn encode_buffer(&mut self, pcm_left: &mut[i16], pcm_right: &mut[i16], mp3_buffer: &mut[u8])
        -> Result<usize, EncodeError> {
        handle_encode_return_code(unsafe {
            lame_sys::lame_encode_buffer(
                self.context, pcm_left.as_mut_ptr(), pcm_right.as_mut_ptr(),
                pcm_left.len() as c_int, mp3_buffer.as_mut_ptr(), mp3_buffer.len() as c_int
            )
        })
    }

    pub fn encode_flush(&mut self, mp3_buffer: &mut[u8]) -> Result<usize, EncodeError> {
        handle_encode_return_code(unsafe {
            lame_sys::lame_encode_flush(self.context, mp3_buffer.as_mut_ptr(), mp3_buffer.len() as c_int)
        })
    }
}

impl Drop for Lame {
    fn drop(&mut self) {
        unsafe {
            lame_sys::lame_close(self.context);
        }
    }
}

fn handle_return_code(code: c_int) -> Result<(), Error> {
    match code {
        0 => Ok(()),
        err => Err(Error::Unknown(err))
    }
}

fn handle_encode_return_code(code: c_int) -> Result<usize, EncodeError> {
    match code {
        -1 => Err(EncodeError::Mp3BufferTooSmall),
        -2 => Err(EncodeError::MallocProblem),
        -3 => Err(EncodeError::InitParamsNotCalled),
        -4 => Err(EncodeError::PsychoAcousticProblem),
        _ => {
            if code < 0 {
                Err(EncodeError::Unknown(code))
            } else {
                Ok(code as usize)
            }
        }
    }
}

#[derive(Debug)]
pub enum Error {
    InitializationFailed,
    Unknown(c_int)
}

#[derive(Debug)]
pub enum EncodeError {
    Mp3BufferTooSmall,
    MallocProblem,
    InitParamsNotCalled,
    PsychoAcousticProblem,
    Unknown(c_int)
}
