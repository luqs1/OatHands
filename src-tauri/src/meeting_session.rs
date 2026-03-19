use crate::portable::app_data_dir;
use crate::settings::get_settings;
use anyhow::Result;
use chrono::{DateTime, Utc};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const MEETINGS_DIR: &str = "meetings";
const TRANSCRIPTION_SAMPLE_RATE: u32 = 16000;
const FLUSH_SAMPLES: usize = 48000;

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct Utterance {
    pub id: String,
    pub speaker: Speaker,
    pub text: String,
    pub timestamp_ms: i64,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum Speaker {
    You,
    Them,
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct MeetingSessionSummary {
    pub id: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub duration_secs: u64,
    pub utterance_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SessionMeta {
    id: String,
    started_at: String,
    ended_at: Option<String>,
    duration_secs: u64,
}

struct StreamingMicCapture {
    _stream: cpal::Stream,
}

impl StreamingMicCapture {
    fn new(
        device: Option<cpal::Device>,
        sample_cb: Arc<dyn Fn(Vec<f32>) + Send + Sync + 'static>,
    ) -> Result<Self> {
        let host = crate::audio_toolkit::get_cpal_host();
        let device = device
            .or_else(|| host.default_input_device())
            .ok_or_else(|| anyhow::anyhow!("No input device"))?;

        let name = device.name().unwrap_or_else(|_| "Unknown".into());

        let config = device
            .supported_input_configs()
            .map_err(|e| anyhow::anyhow!("No input configs: {e}"))?
            .find(|c| c.min_sample_rate().0 <= 48000 && c.max_sample_rate().0 >= 48000)
            .or_else(|| device.supported_input_configs().ok()?.into_iter().next())
            .ok_or_else(|| anyhow::anyhow!("No supported config"))?;

        let channels = config.channels();
        let config = config.with_sample_rate(cpal::SampleRate(48000));

        info!(
            "Streaming mic: {} | rate: 48000 | channels: {}",
            name, channels
        );

        let mut eos_sent = false;
        let stream = device
            .build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if eos_sent {
                        return;
                    }

                    let frame_count = data.len() / channels as usize;
                    let mut mono = Vec::with_capacity(frame_count);

                    for frame in data.chunks_exact(channels as usize) {
                        mono.push(frame.iter().sum::<f32>() / channels as f32);
                    }

                    if !mono.is_empty() {
                        sample_cb(mono);
                    }
                },
                |err| error!("Streaming mic error: {err}"),
                None,
            )
            .map_err(|e| anyhow::anyhow!("Failed to build stream: {e}"))?;

        stream
            .play()
            .map_err(|e| anyhow::anyhow!("Failed to start stream: {e}"))?;

        Ok(Self { _stream: stream })
    }
}

pub struct MeetingSessionManager {
    app_handle: AppHandle,
    is_active: Arc<AtomicBool>,
    session_id: Arc<Mutex<Option<String>>>,
    session_path: Arc<Mutex<Option<PathBuf>>>,
    transcript_file: Arc<Mutex<Option<BufWriter<File>>>>,
    mic_capture: Arc<Mutex<Option<StreamingMicCapture>>>,
    system_capture: Arc<Mutex<Option<crate::audio_toolkit::SystemAudioCapture>>>,
    mic_buffer: Arc<Mutex<Vec<f32>>>,
    system_buffer: Arc<Mutex<Vec<f32>>>,
    transcription_manager: Arc<crate::managers::transcription::TranscriptionManager>,
    session_start_ms: Arc<Mutex<Option<i64>>>,
}

impl MeetingSessionManager {
    pub fn new(
        app_handle: &AppHandle,
        transcription_manager: Arc<crate::managers::transcription::TranscriptionManager>,
    ) -> Result<Self> {
        Ok(Self {
            app_handle: app_handle.clone(),
            is_active: Arc::new(AtomicBool::new(false)),
            session_id: Arc::new(Mutex::new(None)),
            session_path: Arc::new(Mutex::new(None)),
            transcript_file: Arc::new(Mutex::new(None)),
            mic_capture: Arc::new(Mutex::new(None)),
            system_capture: Arc::new(Mutex::new(None)),
            mic_buffer: Arc::new(Mutex::new(Vec::with_capacity(FLUSH_SAMPLES * 2))),
            system_buffer: Arc::new(Mutex::new(Vec::with_capacity(FLUSH_SAMPLES * 2))),
            transcription_manager,
            session_start_ms: Arc::new(Mutex::new(None)),
        })
    }

    fn session_dir(&self) -> Result<PathBuf> {
        let app_data = app_data_dir(&self.app_handle)?;
        let meetings_dir = app_data.join(MEETINGS_DIR);
        if !meetings_dir.exists() {
            fs::create_dir_all(&meetings_dir)?;
        }
        Ok(meetings_dir)
    }

    fn write_transcript_line(&self, speaker: &Speaker, text: &str, timestamp_ms: i64) {
        let utterance = Utterance {
            id: format!("u_{}", timestamp_ms),
            speaker: speaker.clone(),
            text: text.to_string(),
            timestamp_ms,
            duration_ms: 0,
        };

        if let Some(ref mut writer) = *self.transcript_file.lock().unwrap() {
            let line = serde_json::to_string(&utterance).unwrap_or_default();
            let _ = writeln!(writer, "{}", line);
            let _ = writer.flush();
        }
    }

    fn transcribe_buffer(
        buffer: Vec<f32>,
        speaker: Speaker,
        app_handle: &AppHandle,
        tm: Arc<crate::managers::transcription::TranscriptionManager>,
    ) {
        if buffer.len() < 8000 {
            debug!(
                "[MEETING-TRANSCRIBE] Buffer too small ({} samples), skipping",
                buffer.len()
            );
            return;
        }

        let start_ms = Utc::now().timestamp_millis()
            - (buffer.len() as i64 * 1000 / TRANSCRIPTION_SAMPLE_RATE as i64);

        info!(
            "[MEETING-TRANSCRIBE] Transcribing {} samples for {:?}...",
            buffer.len(),
            speaker
        );

        match tm.transcribe(buffer) {
            Ok(text) if !text.trim().is_empty() => {
                let utterance = Utterance {
                    id: format!("u_{}", start_ms),
                    speaker,
                    text: text.trim().to_string(),
                    timestamp_ms: start_ms,
                    duration_ms: 0,
                };
                info!(
                    "[MEETING-TRANSCRIBE] Got transcription: {} chars",
                    utterance.text.len()
                );
                let _ = app_handle.emit("meeting-utterance", &utterance);
                info!("[{:?}] {}", utterance.speaker, utterance.text);
            }
            Ok(_) => {
                debug!("[MEETING-TRANSCRIBE] Empty transcription result");
            }
            Err(e) => {
                warn!("[MEETING-TRANSCRIBE] Transcription error: {}", e);
            }
        }
    }

    pub fn start_meeting(&self) -> Result<String> {
        if self.is_active.load(Ordering::SeqCst) {
            error!("[MEETING] start_meeting called but meeting already active");
            return Err(anyhow::anyhow!("A meeting is already active"));
        }

        info!("[MEETING] Creating new meeting session...");

        let session_id = format!("meeting_{}", Utc::now().format("%Y-%m-%d_%H-%M-%S"));
        let session_dir = self.session_dir()?.join(&session_id);

        info!("[MEETING] Session directory: {:?}", session_dir);

        fs::create_dir_all(&session_dir)?;
        info!("[MEETING] Session directory created");

        let start_ms = Utc::now().timestamp_millis();
        let started_at = DateTime::from_timestamp_millis(start_ms)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_default();

        let meta = SessionMeta {
            id: session_id.clone(),
            started_at: started_at.clone(),
            ended_at: None,
            duration_secs: 0,
        };
        let meta_path = session_dir.join("meta.json");
        let mut meta_file = File::create(&meta_path)?;
        serde_json::to_writer_pretty(&mut meta_file, &meta)?;
        info!("[MEETING] Meta file written");

        let transcript_path = session_dir.join("transcript.jsonl");
        let transcript_file = File::create(&transcript_path)?;
        *self.transcript_file.lock().unwrap() = Some(BufWriter::new(transcript_file));
        info!("[MEETING] Transcript file opened");

        *self.session_id.lock().unwrap() = Some(session_id.clone());
        *self.session_path.lock().unwrap() = Some(session_dir);
        *self.session_start_ms.lock().unwrap() = Some(start_ms);

        self.is_active.store(true, Ordering::SeqCst);

        info!("[MEETING] Starting mic stream...");
        if let Err(e) = self.start_mic_stream() {
            error!("[MEETING] Failed to start mic stream: {}", e);
            let _ = self
                .app_handle
                .emit("meeting-log", &format!("ERROR: Mic stream failed: {}", e));
        } else {
            info!("[MEETING] Mic stream started successfully");
        }

        info!("[MEETING] Starting system audio stream...");
        if let Err(e) = self.start_system_stream() {
            error!("[MEETING] Failed to start system stream: {}", e);
            let _ = self
                .app_handle
                .emit("meeting-log", &format!("ERROR: System audio failed: {}", e));
        } else {
            info!("[MEETING] System stream started successfully");
        }

        info!("[MEETING] Meeting started: {}", session_id);
        let _ = self.app_handle.emit("meeting-started", &session_id);
        let _ = self
            .app_handle
            .emit("meeting-log", &format!("Meeting started: {}", session_id));

        Ok(session_id)
    }

    fn start_mic_stream(&self) -> Result<()> {
        let settings = get_settings(&self.app_handle);
        let host = crate::audio_toolkit::get_cpal_host();

        info!(
            "[MEETING-MIC] Looking for microphone: {:?}",
            settings.selected_microphone
        );

        let device = settings.selected_microphone.as_ref().and_then(|name| {
            host.input_devices()
                .ok()?
                .find(|d| d.name().as_ref().map(|n| n == name).unwrap_or(false))
        });

        if device.is_none() {
            warn!(
                "[MEETING-MIC] Selected microphone '{}' not found, using default",
                settings
                    .selected_microphone
                    .as_ref()
                    .unwrap_or(&"unknown".to_string())
            );
        }

        let actual_device = device.or_else(|| host.default_input_device());
        if let Some(ref d) = actual_device {
            info!("[MEETING-MIC] Using device: {:?}", d.name());
        } else {
            error!("[MEETING-MIC] No input device available!");
            return Err(anyhow::anyhow!("No microphone available"));
        }

        let mic_buffer = self.mic_buffer.clone();
        let is_active = self.is_active.clone();

        let sample_cb = Arc::new(move |samples: Vec<f32>| {
            if !is_active.load(Ordering::SeqCst) {
                return;
            }
            let mut buf = mic_buffer.lock().unwrap();
            buf.extend(samples);
        }) as Arc<dyn Fn(Vec<f32>) + Send + Sync>;

        let capture = StreamingMicCapture::new(actual_device, sample_cb)?;
        info!("[MEETING-MIC] Stream created successfully");

        let buffer_clone = self.mic_buffer.clone();
        let app_clone = self.app_handle.clone();
        let tm_clone = self.transcription_manager.clone();
        let active_clone = self.is_active.clone();

        thread::spawn(move || {
            info!("[MEETING-MIC] Buffer thread started");
            while active_clone.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_secs(3));

                let buf_len = {
                    let mut buf = buffer_clone.lock().unwrap();
                    let len = buf.len();
                    if len >= FLUSH_SAMPLES {
                        debug!("[MEETING-MIC] Flushing {} samples for transcription", len);
                        let flush = buf.split_off(0);
                        *buf = if buf.capacity() > FLUSH_SAMPLES * 2 {
                            Vec::with_capacity(FLUSH_SAMPLES * 2)
                        } else {
                            std::mem::take(&mut *buf)
                        };
                        drop(buf);
                        let app_clone2 = app_clone.clone();
                        let tm_clone2 = tm_clone.clone();
                        thread::spawn(move || {
                            Self::transcribe_buffer(flush, Speaker::You, &app_clone2, tm_clone2);
                        });
                    }
                    len
                };
                debug!("[MEETING-MIC] Buffer: {} samples", buf_len);
            }

            let remaining: Vec<f32> = buffer_clone.lock().unwrap().drain(..).collect();
            if !remaining.is_empty() {
                debug!(
                    "[MEETING-MIC] Transcribing {} remaining samples",
                    remaining.len()
                );
                let app = app_clone.clone();
                let tm = tm_clone.clone();
                thread::spawn(move || {
                    Self::transcribe_buffer(remaining, Speaker::You, &app, tm);
                });
            }
            info!("[MEETING-MIC] Buffer thread ended");
        });

        *self.mic_capture.lock().unwrap() = Some(capture);
        Ok(())
    }

    fn start_system_stream(&self) -> Result<()> {
        info!("[MEETING-SYSTEM] Initializing system audio capture...");
        let mut capture = crate::audio_toolkit::SystemAudioCapture::new();

        let buffer = self.system_buffer.clone();
        let is_active = self.is_active.clone();

        capture = capture.with_callback(move |samples: Vec<f32>| {
            if !is_active.load(Ordering::SeqCst) {
                return;
            }
            let mut buf = buffer.lock().unwrap();
            buf.extend(samples);
        });

        match capture.open() {
            Ok(()) => {
                info!("[MEETING-SYSTEM] System audio opened successfully");
                if let Some(name) = capture.device_name() {
                    info!("[MEETING-SYSTEM] Device name: {}", name);
                }
            }
            Err(e) => {
                error!("[MEETING-SYSTEM] Failed to open system audio: {}", e);
                let _ = self
                    .app_handle
                    .emit("meeting-log", &format!("ERROR: System audio failed: {}", e));
                return Err(anyhow::anyhow!("Failed to open system audio: {}", e));
            }
        }

        capture.start().ok();
        info!("[MEETING-SYSTEM] System audio capture started");

        let buffer_clone = self.system_buffer.clone();
        let app_clone = self.app_handle.clone();
        let tm = self.transcription_manager.clone();
        let active_clone = self.is_active.clone();

        thread::spawn(move || {
            info!("[MEETING-SYSTEM] Buffer thread started");
            while active_clone.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_secs(3));

                let buf_len = {
                    let mut buf = buffer_clone.lock().unwrap();
                    let len = buf.len();
                    if len >= FLUSH_SAMPLES {
                        debug!(
                            "[MEETING-SYSTEM] Flushing {} samples for transcription",
                            len
                        );
                        let flush = buf.split_off(0);
                        *buf = if buf.capacity() > FLUSH_SAMPLES * 2 {
                            Vec::with_capacity(FLUSH_SAMPLES * 2)
                        } else {
                            std::mem::take(&mut *buf)
                        };
                        drop(buf);
                        let app_clone2 = app_clone.clone();
                        let tm_clone2 = tm.clone();
                        thread::spawn(move || {
                            Self::transcribe_buffer(flush, Speaker::Them, &app_clone2, tm_clone2);
                        });
                    }
                    len
                };
                debug!("[MEETING-SYSTEM] Buffer: {} samples", buf_len);
            }

            let remaining: Vec<f32> = buffer_clone.lock().unwrap().drain(..).collect();
            if !remaining.is_empty() {
                debug!(
                    "[MEETING-SYSTEM] Transcribing {} remaining samples",
                    remaining.len()
                );
                let app = app_clone.clone();
                let tm = tm.clone();
                thread::spawn(move || {
                    Self::transcribe_buffer(remaining, Speaker::Them, &app, tm);
                });
            }
            info!("[MEETING-SYSTEM] Buffer thread ended");
        });

        *self.system_capture.lock().unwrap() = Some(capture);
        Ok(())
    }

    pub fn stop_meeting(&self) -> Result<MeetingSessionSummary> {
        if !self.is_active.load(Ordering::SeqCst) {
            return Err(anyhow::anyhow!("No active meeting to stop"));
        }

        info!("[MEETING] Stopping meeting...");
        self.is_active.store(false, Ordering::SeqCst);

        info!("[MEETING] Stopping system audio capture...");
        if let Some(mut capture) = self.system_capture.lock().unwrap().take() {
            capture.stop();
            capture.close();
        }

        info!("[MEETING] Stopping mic capture...");
        *self.mic_capture.lock().unwrap() = None;

        let start_ms = self
            .session_start_ms
            .lock()
            .unwrap()
            .unwrap_or_else(|| Utc::now().timestamp_millis());
        let end_ms = Utc::now().timestamp_millis();
        let duration_secs = ((end_ms - start_ms) / 1000) as u64;
        let session_id = self.session_id.lock().unwrap().clone().unwrap_or_default();

        if let Some(path) = self.session_path.lock().unwrap().take() {
            let ended_at = DateTime::from_timestamp_millis(end_ms)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default();
            let meta = SessionMeta {
                id: session_id.clone(),
                started_at: DateTime::from_timestamp_millis(start_ms)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default(),
                ended_at: Some(ended_at),
                duration_secs,
            };
            let meta_path = path.join("meta.json");
            if let Ok(mut file) = File::create(&meta_path) {
                let _ = serde_json::to_writer_pretty(&mut file, &meta);
            }
        }

        *self.transcript_file.lock().unwrap() = None;
        *self.session_id.lock().unwrap() = None;

        let utterance_count = self.mic_buffer.lock().unwrap().len() / FLUSH_SAMPLES;

        info!(
            "[MEETING] Meeting stopped: {} ({}s, {} utterances)",
            session_id, duration_secs, utterance_count
        );
        let _ = self.app_handle.emit("meeting-stopped", &session_id);

        Ok(MeetingSessionSummary {
            id: session_id,
            started_at: start_ms,
            ended_at: Some(end_ms),
            duration_secs,
            utterance_count,
        })
    }

    pub fn is_meeting_active(&self) -> bool {
        self.is_active.load(Ordering::SeqCst)
    }

    pub fn get_current_session_id(&self) -> Option<String> {
        self.session_id.lock().unwrap().clone()
    }

    pub fn list_meetings(&self) -> Result<Vec<MeetingSessionSummary>> {
        let meetings_dir = self.session_dir()?;
        let mut summaries = Vec::new();

        for entry in fs::read_dir(meetings_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let meta_path = path.join("meta.json");
            if !meta_path.exists() {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&meta_path) {
                if let Ok(meta) = serde_json::from_str::<SessionMeta>(&content) {
                    let transcript_path = path.join("transcript.jsonl");
                    let utterance_count = if transcript_path.exists() {
                        fs::read_to_string(&transcript_path)
                            .map(|s| s.lines().count())
                            .unwrap_or(0)
                    } else {
                        0
                    };

                    let started_at = DateTime::parse_from_rfc3339(&meta.started_at)
                        .map(|dt| dt.timestamp_millis())
                        .unwrap_or(0);

                    let ended_at = meta.ended_at.as_ref().and_then(|s| {
                        DateTime::parse_from_rfc3339(s)
                            .map(|dt| dt.timestamp_millis())
                            .ok()
                    });

                    summaries.push(MeetingSessionSummary {
                        id: meta.id,
                        started_at,
                        ended_at,
                        duration_secs: meta.duration_secs,
                        utterance_count,
                    });
                }
            }
        }

        summaries.sort_by_key(|s| s.started_at);
        summaries.reverse();
        Ok(summaries)
    }

    pub fn get_meeting_transcript(&self, session_id: &str) -> Result<Vec<Utterance>> {
        let meetings_dir = self.session_dir()?;
        let transcript_path = meetings_dir.join(session_id).join("transcript.jsonl");
        let mut utterances = Vec::new();

        if transcript_path.exists() {
            let content = fs::read_to_string(&transcript_path)?;
            for line in content.lines() {
                if let Ok(u) = serde_json::from_str::<Utterance>(line) {
                    utterances.push(u);
                }
            }
        }

        Ok(utterances)
    }
}
