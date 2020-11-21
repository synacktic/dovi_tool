use std::path::PathBuf;

use indicatif::ProgressBar;

use super::{io, Format};

use io::{DoviReader, DoviWriter};

pub struct Muxer {
    format: Format,
    bl_in: PathBuf,
    el_in: PathBuf,
    output: PathBuf,
}

impl Muxer {
    pub fn new(format: Format, bl_in: PathBuf, el_in: PathBuf, output: PathBuf) -> Self {
        Self {
            format,
            bl_in,
            el_in,
            output
        }
    }

    pub fn process_input(&self, mode: Option<u8>) {
        match self.format {
            Format::Matroska => panic!("unsupported"),
            _ => self.mux_raw_hevc(None, mode),
        };
    }

    pub fn mux_raw_hevc(&self, pb: Option<&ProgressBar>, mode: Option<u8>) {
        let mut bl_reader = DoviReader::new(mode);
        let mut el_reader = DoviReader::new(mode);

        let mut dovi_writer = DoviWriter::new(None, None, None, None);

        match dovi_reader.read_write_from_io(&self.format, &self.input, pb, &mut dovi_writer, None) {
            Ok(_) => (),
            Err(e) => panic!(e),
        }
    }
}
