use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use rsor::Slice;

use crate::audiofile::BoxedError;
use crate::parser::{FileStorage, PlaylistEntry};

pub enum State {
    Playing(u64),
    Seeking(u64),
}

type DataConsumer = rtrb::fixed_chunks::FixedChunkConsumer<f32>;

pub struct FileStreamer {
    ready_consumer: rtrb::Consumer<(u64, DataConsumer)>,
    seek_producer: rtrb::Producer<(u64, DataConsumer)>,
    data_consumer: Option<DataConsumer>,
    reader_thread: Option<thread::JoinHandle<Result<(), BoxedError>>>,
    reader_thread_keep_reading: Arc<AtomicBool>,
    channels: u32,
    blocksize: u32,
    previously_rolling: bool,
    state: State,
    sos: Slice<[f32]>,
}

struct ActiveIter<'a> {
    block_start: u64,
    block_end: u64,
    inner: std::slice::IterMut<'a, PlaylistEntry>,
}

impl<'a> Iterator for ActiveIter<'a> {
    type Item = &'a mut PlaylistEntry;

    fn next(&mut self) -> Option<&'a mut PlaylistEntry> {
        while let Some(entry) = self.inner.next() {
            if entry.begin < self.block_end && self.block_start < (entry.begin + entry.duration) {
                return Some(entry);
            }
        }
        None
    }
}

impl FileStreamer {
    pub fn new(
        mut playlist: Vec<PlaylistEntry>,
        mut file_storage: FileStorage,
        blocksize: u32,
        channels: u32,
        buffer_blocks: u32,
        sleeptime: Duration,
    ) -> FileStreamer {
        let chunksize = blocksize as usize * channels as usize;
        let (mut ready_producer, ready_consumer) = rtrb::RingBuffer::new(1);
        let (seek_producer, mut seek_consumer) = rtrb::RingBuffer::<(u64, DataConsumer)>::new(1);
        let (data_producer, data_consumer) =
            rtrb::RingBuffer::new(buffer_blocks as usize * chunksize as usize);
        let mut data_producer = data_producer.try_fixed_chunk_size(chunksize).unwrap();
        let data_consumer = data_consumer.try_fixed_chunk_size(chunksize).unwrap();

        let reader_thread_keep_reading = Arc::new(AtomicBool::new(true));
        let keep_reading = Arc::clone(&reader_thread_keep_reading);
        let reader_thread = thread::spawn(move || {
            let mut data_consumer = Some(data_consumer);
            let mut current_frame = 0;
            let mut seek_frame = 0;
            let mut sos = Slice::with_capacity(channels as usize);

            while keep_reading.load(Ordering::Acquire) {
                if let Ok((frame, mut queue)) = seek_consumer.pop() {
                    // NB: By owning data_producer, we know that no new items can be written while
                    //     we drain the consumer:
                    while queue.pop_chunk().is_ok() {}
                    data_consumer = Some(queue);
                    current_frame = frame;
                    seek_frame = frame;
                }
                if let Ok(mut chunk) = data_producer.push_chunk() {
                    let target = sos.from_iter_mut(chunk.chunks_mut(blocksize as usize));
                    debug_assert_eq!(target.len(), channels as usize);

                    // NB: Slice from RingBuffer is already filled with zeros

                    let mut active_files = ActiveIter {
                        block_start: current_frame,
                        block_end: current_frame + u64::from(blocksize),
                        inner: playlist.iter_mut(),
                    };
                    // TODO: Is linear search too slow? How long can playlists be?
                    for entry in &mut active_files {
                        let (file, channel_map) = &mut file_storage[entry.idx];
                        let offset = if entry.begin < current_frame {
                            if current_frame == seek_frame {
                                file.seek(current_frame - entry.begin)?;
                            }
                            0
                        } else {
                            file.seek(0)?;
                            (entry.begin - current_frame) as u32
                        };
                        file.fill_channels(&channel_map, blocksize, offset, target)?;
                    }
                    current_frame += u64::from(blocksize);

                    // Make sure the block is queued before data_consumer is sent
                    drop(chunk);

                    if current_frame - seek_frame >= u64::from(buffer_blocks) * u64::from(blocksize)
                    {
                        if let Some(data_consumer) = data_consumer.take() {
                            // There is only one data queue, push() will always succeed
                            ready_producer.push((seek_frame, data_consumer)).unwrap();
                        }
                    }
                } else {
                    thread::sleep(sleeptime);
                }
            }
            Ok(())
        });
        FileStreamer {
            ready_consumer,
            seek_producer,
            data_consumer: None,
            reader_thread: Some(reader_thread),
            reader_thread_keep_reading,
            channels,
            blocksize,
            previously_rolling: false,
            state: State::Seeking(0),
            sos: Slice::with_capacity(channels as usize),
        }
    }

    pub fn channels(&self) -> u32 {
        self.channels
    }

    /// `target` will be filled with zeros in case of an error.
    pub fn get_data(
        &mut self,
        target: &mut [&mut [f32]],
        rolling: bool,
    ) -> Result<(), StreamingError> {
        let previously = self.previously_rolling;
        if !rolling && !previously {
            fill_with_zeros(target);
        } else if let Some(ref mut queue) = self.data_consumer {
            if let Ok(chunk) = queue.pop_chunk() {
                let source = self.sos.from_iter(chunk.chunks(self.blocksize as usize));
                debug_assert_eq!(source.len(), self.channels as usize);
                for (source, target) in source.iter().zip(target) {
                    if rolling && !previously {
                        // Fade In
                        let ramp = 1..;
                        for (r, (s, t)) in ramp.zip(source.iter().zip(target.iter_mut())) {
                            *t = s * r as f32 / self.blocksize as f32;
                        }
                    } else if !rolling && previously {
                        // Fade Out
                        let ramp = (1..=self.blocksize).rev();
                        for (r, (s, t)) in ramp.zip(source.iter().zip(target.iter_mut())) {
                            *t = s * r as f32 / self.blocksize as f32;
                        }
                    } else {
                        // No Fade
                        target.copy_from_slice(source);
                    };
                }
                if let State::Playing(f) = self.state {
                    self.state = State::Playing(f + self.blocksize as u64);
                }
            } else {
                fill_with_zeros(target);
                return Err(StreamingError::EmptyBuffer);
            }
        } else {
            fill_with_zeros(target);
            return Err(StreamingError::IncompleteSeek);
        };
        self.previously_rolling = rolling;
        if let State::Seeking(frame) = self.state {
            if rolling {
                return Err(StreamingError::SeekWhileRolling);
            }
            let _ = self.seek(frame);
        }
        Ok(())
    }

    #[must_use]
    pub fn seek(&mut self, frame: u64) -> bool {
        if let State::Playing(f) = self.state {
            if f == frame {
                return true;
            }
        }
        self.state = State::Seeking(frame);
        if self.previously_rolling {
            // Don't seek yet; get_data() fades out and calls seek afterwards
            return false;
        }
        if self.data_consumer.is_none() {
            // NB: There can never be more than one message
            if let Ok((ready_frame, queue)) = self.ready_consumer.pop() {
                self.data_consumer = Some(queue);
                if ready_frame == frame {
                    self.state = State::Playing(frame);
                    return true;
                }
            }
        }
        if let Some(queue) = self.data_consumer.take() {
            self.seek_producer.push((frame, queue)).unwrap();
        }
        false
    }
}

impl Drop for FileStreamer {
    fn drop(&mut self) {
        self.reader_thread_keep_reading
            .store(false, Ordering::Release);
        // TODO: handle error from closure? log errors?
        self.reader_thread.take().unwrap().join().unwrap().unwrap();
    }
}

fn fill_with_zeros(target: &mut [&mut [f32]]) {
    for slice in target.iter_mut() {
        // TODO: use slice::fill() once stabilized:
        //slice.fill(0.0f32);
        for elem in slice.iter_mut() {
            *elem = 0.0f32;
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum StreamingError {
    #[error("Empty file-streaming buffer")]
    EmptyBuffer,
    #[error("Bug: The seek function must be called until it returns true")]
    IncompleteSeek,
    #[error("Bug: Seeking while rolling is not supported")]
    SeekWhileRolling,
}
