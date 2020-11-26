use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;

use ansi_term::Colour::Red;
use indicatif::ProgressBar;
use read_byte_slice::{ByteSliceIter, FallibleStreamingIterator};
use nom::{IResult, bytes::complete::take_until};

use super::rpu::parse_dovi_rpu;
use super::Format;

const NAL_START_CODE: &[u8] = &[0, 0, 0, 1];
const HEADER_LEN: usize = 4;

pub struct DoviReader {
    mode: Option<u8>,
    skip_next: usize,
}

pub struct DoviWriter {
    bl_writer: Option<BufWriter<File>>,
    el_writer: Option<BufWriter<File>>,
    rpu_writer: Option<BufWriter<File>>,
}

#[derive(Debug)]
pub enum ChunkType {
    BLChunk,
    ELChunk,
    RPUChunk,
}

impl DoviWriter {
    pub fn new(
        bl_out: Option<&PathBuf>,
        el_out: Option<&PathBuf>,
        rpu_out: Option<&PathBuf>,
    ) -> DoviWriter {
        let chunk_size = 100_000;
        let bl_writer = if let Some(bl_out) = bl_out {
            Some(BufWriter::with_capacity(
                chunk_size * 2,
                File::create(bl_out).expect("Can't create file"),
            ))
        } else {
            None
        };

        let el_writer = if let Some(el_out) = el_out {
            Some(BufWriter::with_capacity(
                chunk_size,
                File::create(el_out).expect("Can't create file"),
            ))
        } else {
            None
        };

        let rpu_writer = if let Some(rpu_out) = rpu_out {
            Some(BufWriter::with_capacity(
                chunk_size,
                File::create(rpu_out).expect("Can't create file"),
            ))
        } else {
            None
        };

        DoviWriter {
            bl_writer,
            el_writer,
            rpu_writer,
        }
    }
}

impl DoviReader {
    pub fn new(mode: Option<u8>) -> DoviReader {
        DoviReader {
            mode,
            skip_next: 0,
        }
    }

    pub fn take_until_nal(data: &[u8]) -> IResult<&[u8], &[u8]> {
        take_until(NAL_START_CODE)(data)
    }

    pub fn read_write_from_io(
        &mut self,
        format: &Format,
        input: &PathBuf,
        pb: Option<&ProgressBar>,
        dovi_writer: &mut DoviWriter,
    ) -> Result<(), std::io::Error> {
        //BufReader & BufWriter
        let stdin = std::io::stdin();
        let mut reader = Box::new(stdin.lock()) as Box<dyn BufRead>;

        if let Format::Raw = format {
            let file = File::open(input)?;
            reader = Box::new(BufReader::with_capacity(100_000, file));
        }

        //Byte chunk iterator
        let mut iter = ByteSliceIter::new(reader, 100_000);

        let mut current_chunk_type: Option<ChunkType> = None;
        let mut consumed = 0;
        let mut no_next_nal = false;

        let mut nal_type_index = HEADER_LEN;

        let mut current_rpu: Vec<u8> = Vec::with_capacity(1024);

        // Loop over chunks
        while let Some(read_data) = iter.next()? {
            'chunk: loop {
                match Self::take_until_nal(&read_data[consumed..]) {
                    Ok(nal) => {
                        let nal_data = nal.0;

                        // New bytes input chunk, write the rest into previous writer
                        if consumed == 0 {
                            if let Some(ref chunk_type) = current_chunk_type {
                                let previous_nal_data = nal.1;

                                // We need a complete RPU to parse
                                // Don't write the header as it was already done
                                match chunk_type {
                                    ChunkType::RPUChunk => {
                                        current_rpu.extend_from_slice(previous_nal_data);
                                        self.write_nal_data(dovi_writer, chunk_type, &current_rpu, true)?;

                                        current_rpu.clear();
                                    },
                                    ChunkType::ELChunk => {
                                        self.write_nal_data(dovi_writer, chunk_type, &previous_nal_data[self.skip_next..], false)?;

                                        self.skip_next = 0;
                                    }
                                    _ => self.write_nal_data(dovi_writer, chunk_type, previous_nal_data, false)?,
                                }

                                // Consumed the previous data
                                consumed += previous_nal_data.len();
                            }
                        } else if nal_data.len() == HEADER_LEN {
                            // EL needs to remove the fake type
                            self.skip_next = 2;

                            nal_type_index = 0;
                            consumed = 0;
                            no_next_nal = false;
                            break 'chunk;
                        }
    
                        let nal_type = nal_data[nal_type_index] >> 1;

                        match nal_type {
                            62 => current_chunk_type = Some(ChunkType::RPUChunk),
                            63 => current_chunk_type = Some(ChunkType::ELChunk),
                            _ => current_chunk_type = Some(ChunkType::BLChunk),
                        };

                        // Writer header into correct output, reset type index
                        if nal_type_index == 0 {
                            // Only have the header in this chunk
                            if let Some(ref chunk_type) = current_chunk_type {
                                self.write_nal_header(dovi_writer, chunk_type)?;
                            }

                            nal_type_index = HEADER_LEN;
                        }
    
                        // Find the next nal, get the length of the previous data
                        // If no match, the size is the whole slice
                        let size = match Self::take_until_nal(&nal_data[HEADER_LEN..]) {
                            Ok(next_nal) => next_nal.1.len() + HEADER_LEN,
                            _ => {
                                no_next_nal = true;
                                nal_data.len()
                            },
                        };

                        // Consumed the size or remaining
                        consumed += size;

                        // At the end of chunk, we don't write if it's a RPU and not complete
                        if nal_type == 62 && nal_data[size - 1] != 0x80 {
                            current_rpu.extend_from_slice(&nal_data[..size]);

                            consumed = 0;
                            no_next_nal = false;
                            break 'chunk;
                        }

                        // Write full NAL
                        if let Some(ref chunk_type) =  current_chunk_type {
                            self.write_nal_data(dovi_writer, chunk_type, &nal_data[..size], true)?;
                        }

                        if no_next_nal {
                            consumed = 0;
                            no_next_nal = false;
                            break 'chunk;
                        }
                    },
                    Err(nom::Err::Error(_)) => {
                        // No match for this chunk at all, write it all as previous type
                        if consumed == 0 {
                            if let Some(ref chunk_type) = current_chunk_type {
                                self.write_nal_data(dovi_writer, chunk_type, &read_data, false)?;
                            }
                        }

                        consumed = 0;
                        no_next_nal = false;

                        break 'chunk;
                    }
                    Err(e) => panic!("{:?}", e),
                }
            }
        }

        if let Some(ref mut bl_writer) = dovi_writer.bl_writer {
            bl_writer.flush()?;
        }

        if let Some(ref mut el_writer) = dovi_writer.el_writer {
            el_writer.flush()?;
        }

        if let Some(ref mut rpu_writer) = dovi_writer.rpu_writer {
            rpu_writer.flush()?;
        }

        Ok(())
    }

    fn write_nal_data(&mut self, dovi_writer: &mut DoviWriter, chunk_type: &ChunkType, data: &[u8], write_header: bool) -> Result<(), std::io::Error> {
        let data = if write_header {
            self.write_nal_header(dovi_writer, chunk_type)?;

            &data[HEADER_LEN..]
        } else {
            data
        };

        match chunk_type {
            ChunkType::BLChunk => {
                if let Some(ref mut bl_writer) = dovi_writer.bl_writer {
                    bl_writer.write(&data)?;
                }
            }
            ChunkType::ELChunk => {
                if let Some(ref mut el_writer) = dovi_writer.el_writer {
                    let skip_write = if data.len() <= 2 {
                        true
                    } else {
                        false
                    };

                    // Partial chunks should be complete, otherwise trim fake nal_type
                    if !skip_write {
                        if write_header {
                            el_writer.write(&data[2..])?;
                        } else {
                            el_writer.write(&data)?;
                        }
                    } else {
                        self.skip_next = 2 - data.len();
                    }
                }
            }
            ChunkType::RPUChunk => {
                // Always complete RPUs

                // No mode: Copy
                // Mode 0: Parse, untouched
                // Mode 1: to MEL
                // Mode 2: to 8.1
                if let Some(mode) = self.mode {
                    match parse_dovi_rpu(&data) {
                        Ok(mut dovi_rpu) => {
                            let modified_data = dovi_rpu.write_rpu_data(mode);

                            if let Some(ref mut rpu_writer) = dovi_writer.rpu_writer {
                                // RPU for x265, remove 0x7C01
                                rpu_writer.write(&modified_data[2..])?;
                            } else if let Some(ref mut el_writer) = dovi_writer.el_writer {
                                el_writer.write(&modified_data)?;
                            }
                        }
                        Err(e) => panic!("{}", Red.paint(e)),
                    }
                } else if let Some(ref mut rpu_writer) = dovi_writer.rpu_writer {
                    // RPU for x265, remove 0x7C01
                    rpu_writer.write(&data[2..])?;
                } else if let Some(ref mut el_writer) = dovi_writer.el_writer {
                    el_writer.write(&data)?;
                }
            }
        }

        Ok(())
    }

    fn write_nal_header(&mut self, dovi_writer: &mut DoviWriter, chunk_type: &ChunkType) -> Result<(), std::io::Error> {
        match chunk_type {
            ChunkType::BLChunk => {
                if let Some(ref mut bl_writer) = dovi_writer.bl_writer {
                    bl_writer.write(NAL_START_CODE)?;
                }
            }
            ChunkType::ELChunk => {
                if let Some(ref mut el_writer) = dovi_writer.el_writer {
                    el_writer.write(NAL_START_CODE)?;
                }
            }
            ChunkType::RPUChunk => {
                if let Some(ref mut rpu_writer) = dovi_writer.rpu_writer {
                    rpu_writer.write(NAL_START_CODE)?;
                } else if let Some(ref mut el_writer) = dovi_writer.el_writer {
                    el_writer.write(NAL_START_CODE)?;
                }
            }
        }

        Ok(())
    }
}
