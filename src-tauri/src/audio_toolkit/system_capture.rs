use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use log::{debug, error, info};
use std::{
    io::Error,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};

enum Cmd {
    Start,
    Stop,
    Shutdown,
}

enum AudioChunk {
    Samples(Vec<f32>),
    EndOfStream,
}

pub struct SystemAudioCapture {
    device: Option<cpal::Device>,
    cmd_tx: Option<mpsc::Sender<Cmd>>,
    worker_handle: Option<std::thread::JoinHandle<()>>,
    on_samples: Option<Arc<dyn Fn(Vec<f32>) + Send + Sync + 'static>>,
}

impl SystemAudioCapture {
    pub fn new() -> Self {
        Self {
            device: None,
            cmd_tx: None,
            worker_handle: None,
            on_samples: None,
        }
    }

    pub fn with_callback<F>(mut self, cb: F) -> Self
    where
        F: Fn(Vec<f32>) + Send + Sync + 'static,
    {
        self.on_samples = Some(Arc::new(cb));
        self
    }

    fn find_output_device() -> Option<cpal::Device> {
        let host = crate::audio_toolkit::get_cpal_host();

        #[cfg(target_os = "windows")]
        {
            if let Some(device) = host.default_output_device() {
                debug!("Found Windows loopback device: {:?}", device.name());
                return Some(device);
            }
        }

        #[cfg(target_os = "linux")]
        {
            for device in host.output_devices().ok()? {
                if let Ok(name) = device.name() {
                    let lower = name.to_lowercase();
                    if lower.contains("hdmi")
                        || lower.contains("speaker")
                        || lower.contains("analog")
                        || lower.contains("usb")
                    {
                        debug!("Found Linux system audio device: {}", name);
                        return Some(device);
                    }
                }
            }
            if let Some(device) = host.default_output_device() {
                return Some(device);
            }
        }

        #[cfg(target_os = "macos")]
        {
            if let Some(device) = host.default_output_device() {
                debug!(
                    "Using macOS default output (aggregate device recommended for true loopback): {:?}",
                    device.name()
                );
                return Some(device);
            }
        }

        None
    }

    pub fn open(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.worker_handle.is_some() {
            return Ok(());
        }

        let device = Self::find_output_device().ok_or_else(|| {
            Error::new(std::io::ErrorKind::NotFound, "No system audio device found")
        })?;

        let device_name = device.name().unwrap_or_else(|_| "Unknown".into());
        let thread_device = device.clone();

        let (sample_tx, sample_rx) = mpsc::channel::<AudioChunk>();
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), String>>(1);

        let on_samples = self.on_samples.clone();

        let worker = thread::spawn(move || {
            let stop_flag = Arc::new(AtomicBool::new(false));

            let result: Result<(), String> = (|| {
                let config = thread_device
                    .supported_output_configs()
                    .map_err(|e| format!("No output configs: {e}"))?
                    .find(|c| c.sample_format() == cpal::SampleFormat::F32)
                    .or_else(|| {
                        thread_device
                            .supported_output_configs()
                            .ok()?
                            .into_iter()
                            .next()
                    })
                    .ok_or("No supported output config")?;

                let sample_rate = config.min_sample_rate().0;
                let channels = config.channels() as usize;
                let config = config.with_sample_rate(cpal::SampleRate(sample_rate));

                info!(
                    "System audio: {} | rate: {} | channels: {}",
                    device_name, sample_rate, channels
                );

                let mut output_buffer = Vec::new();
                let mut eos_sent = false;
                let stop_flag_stream = stop_flag.clone();

                let stream = thread_device
                    .build_input_stream(
                        &config.clone().into(),
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            if stop_flag_stream.load(Ordering::Relaxed) {
                                if !eos_sent {
                                    let _ = sample_tx.send(AudioChunk::EndOfStream);
                                    eos_sent = true;
                                }
                                return;
                            }
                            eos_sent = false;

                            output_buffer.clear();

                            let frame_count = data.len() / channels;
                            output_buffer.reserve(frame_count);

                            if channels == 1 {
                                output_buffer.extend_from_slice(data);
                            } else {
                                for frame in data.chunks_exact(channels) {
                                    let mono =
                                        frame.iter().map(|&s| s).sum::<f32>() / channels as f32;
                                    output_buffer.push(mono);
                                }
                            }

                            let _ = sample_tx.send(AudioChunk::Samples(output_buffer.clone()));
                        },
                        |err| error!("System audio stream error: {err}"),
                        None,
                    )
                    .map_err(|e| format!("Failed to build stream: {e}"))?;

                stream
                    .play()
                    .map_err(|e| format!("Failed to start stream: {e}"))?;

                let _ = init_tx
                    .send(Ok(()))
                    .map_err(|e| format!("Send error: {}", e))?;

                let mut running = false;
                let mut flush_buf = Vec::new();
                let flush_interval_samples = (sample_rate * 3) as usize;
                let mut sample_count = 0usize;

                loop {
                    match cmd_rx.recv() {
                        Ok(Cmd::Start) => {
                            stop_flag.store(false, Ordering::Relaxed);
                            running = true;
                            flush_buf.clear();
                            sample_count = 0;
                            debug!("System audio capture started");
                        }
                        Ok(Cmd::Stop) => {
                            stop_flag.store(true, Ordering::Relaxed);
                            running = false;
                            debug!("System audio capture stopped");
                        }
                        Ok(Cmd::Shutdown) => {
                            debug!("System audio capture shutting down");
                            return Ok(());
                        }
                        Err(_) => return Ok(()),
                    }

                    while running {
                        match sample_rx.recv_timeout(Duration::from_millis(100)) {
                            Ok(AudioChunk::Samples(samples)) => {
                                let len = samples.len();
                                if let Some(ref cb) = on_samples {
                                    cb(samples.clone());
                                }
                                flush_buf.extend(samples);
                                sample_count += len;

                                if sample_count >= flush_interval_samples {
                                    let chunk = flush_buf.split_off(0);
                                    sample_count = 0;
                                    let remaining: Vec<f32> = flush_buf.iter().copied().collect();
                                    flush_buf = remaining;
                                    if let Some(ref cb) = on_samples {
                                        cb(chunk);
                                    }
                                }
                            }
                            Ok(AudioChunk::EndOfStream) => {
                                debug!("End of stream received");
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => break,
                            Err(_) => return Ok(()),
                        }

                        if cmd_rx.try_recv().is_ok() {
                            break;
                        }
                    }
                }

                Ok(())
            })();

            if let Err(e) = result {
                error!("System audio capture error: {}", e);
                let _ = init_tx.send(Err(e));
            }
        });

        match init_rx.recv() {
            Ok(Ok(())) => {
                self.device = Some(device);
                self.cmd_tx = Some(cmd_tx);
                self.worker_handle = Some(worker);
                Ok(())
            }
            Ok(Err(e)) => {
                let _ = worker.join();
                Err(Box::new(Error::new(std::io::ErrorKind::Other, e)))
            }
            Err(e) => {
                let _ = worker.join();
                Err(Box::new(Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to initialize system audio worker: {}", e),
                )))
            }
        }
    }

    pub fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(ref tx) = self.cmd_tx {
            tx.send(Cmd::Start)?;
        }
        Ok(())
    }

    pub fn stop(&self) {
        if let Some(ref tx) = self.cmd_tx {
            let _ = tx.send(Cmd::Stop);
        }
    }

    pub fn close(&mut self) {
        if let Some(tx) = self.cmd_tx.take() {
            let _ = tx.send(Cmd::Shutdown);
        }
        if let Some(h) = self.worker_handle.take() {
            let _ = h.join();
        }
        self.device = None;
    }

    pub fn device_name(&self) -> Option<String> {
        self.device.as_ref().and_then(|d| d.name().ok())
    }
}

impl Drop for SystemAudioCapture {
    fn drop(&mut self) {
        self.close();
    }
}
