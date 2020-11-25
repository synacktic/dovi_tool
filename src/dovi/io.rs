use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;

use ansi_term::Colour::Red;
use indicatif::ProgressBar;
use read_byte_slice::{ByteSliceIter, FallibleStreamingIterator};
use nom::{error::ErrorKind, IResult, bytes::complete::take_until};

use super::rpu::parse_dovi_rpu;
use super::Format;

const NAL_START_CODE: &[u8] = &[0, 0, 1];

pub struct DoviReader {
    out_nal_header: Vec<u8>,
    mode: Option<u8>,
}

pub struct DoviWriter {
    bl_writer: Option<BufWriter<File>>,
    el_writer: Option<BufWriter<File>>,
    rpu_writer: Option<BufWriter<File>>,
}

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
            out_nal_header: vec![0, 0, 0, 1],
            mode,
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

        while let Ok(Some(read_data)) = iter.next() {
            'chunk: loop {
                match Self::take_until_nal(&read_data[consumed..]) {
                    Ok(nal) => {
                        if let Some(ref chunk_type) = current_chunk_type {
                            let previous_nal_data = nal.1;
    
                            self.write_nal_data(dovi_writer, chunk_type, previous_nal_data)?;
                        }
    
                        let nal_data = nal.0;
                        let nal_type = nal_data[3] >> 1;
    
                        consumed += nal_data.len();
    
                        match nal_type {
                            62 => { // RPU
                                current_chunk_type = Some(ChunkType::RPUChunk);
                            },
                            63 => { // EL
                                //start = 5;
                                current_chunk_type = Some(ChunkType::ELChunk);  
                            },
                            _ => { // BL
                                current_chunk_type = Some(ChunkType::BLChunk);
                            }
                        }
    
                        if let Some(ref chunk_type) =  current_chunk_type {
                            if let Some(ref mut bl_writer) = dovi_writer.bl_writer {
                                bl_writer.write(&self.out_nal_header)?;
                            } else if let Some(ref mut el_writer) = dovi_writer.el_writer {
                                el_writer.write(&self.out_nal_header)?;
                            } else if let Some(ref mut rpu_writer) = dovi_writer.rpu_writer {
                                rpu_writer.write(&self.out_nal_header)?;
                            }
    
                            let trimmed_data = match chunk_type {
                                ChunkType::ELChunk => &nal_data[5..],
                                _ => &nal_data[3..]
                            };
    
                            self.write_nal_data(dovi_writer, chunk_type, trimmed_data)?;
                        }
                    },
                    Err(nom::Err::Error(e)) => {
                        if let Some(ref chunk_type) = current_chunk_type {
                            self.write_nal_data(dovi_writer, chunk_type, e.input)?;
                        }

                        consumed = 0;
                        break 'chunk;
                    }
                    Err(e) => {
                        println!("{:?}", e.to_string());
                    }
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

    fn write_nal_data(&mut self, dovi_writer: &mut DoviWriter, chunk_type: &ChunkType, data: &[u8]) -> Result<(), std::io::Error> {
        
        match chunk_type {
            ChunkType::BLChunk => {
                if let Some(ref mut bl_writer) = dovi_writer.bl_writer {
                    bl_writer.write(&data)?;
                }
            }
            ChunkType::ELChunk => {
                if let Some(ref mut el_writer) = dovi_writer.el_writer {
                    el_writer.write(&data)?;
                }
            }
            ChunkType::RPUChunk => {
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
}
