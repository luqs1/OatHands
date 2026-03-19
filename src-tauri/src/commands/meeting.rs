use crate::llm_client;
use crate::meeting_session::{MeetingSessionManager, MeetingSessionSummary, Utterance};
use crate::settings::get_settings;
use log::{error, info};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex as TokioMutex;

static NOTES_CANCEL_FLAG: once_cell::sync::Lazy<Arc<TokioMutex<bool>>> =
    once_cell::sync::Lazy::new(|| Arc::new(TokioMutex::new(false)));

#[derive(Clone, Serialize, Deserialize, Type)]
pub struct MeetingSettings {
    pub provider_id: String,
    pub model: String,
    pub api_key: String,
    pub system_prompt: String,
    pub template: String,
}

impl Default for MeetingSettings {
    fn default() -> Self {
        Self {
            provider_id: "openrouter".to_string(),
            model: "openai/gpt-4o-mini".to_string(),
            api_key: String::new(),
            system_prompt: Self::default_system_prompt(),
            template: Self::default_template(),
        }
    }
}

impl MeetingSettings {
    fn default_system_prompt() -> String {
        "You are a professional meeting notes assistant. Based on the transcript of a meeting, generate comprehensive, well-structured notes. Include: key discussion points, decisions made, action items (with owners if mentioned), questions raised, and any next steps. Format with clear headings and bullet points. Be concise but thorough.".to_string()
    }

    fn default_template() -> String {
        r#"# Meeting Notes

## Key Discussion Points

## Decisions Made

## Action Items
- [ ]

## Questions Raised

## Next Steps
"#
        .to_string()
    }
}

#[tauri::command]
#[specta::specta]
pub async fn start_meeting(
    app: AppHandle,
    manager: State<'_, Arc<MeetingSessionManager>>,
) -> Result<String, String> {
    info!("[MEETING] start_meeting called");
    match manager.start_meeting() {
        Ok(session_id) => {
            info!("[MEETING] Meeting started successfully: {}", session_id);
            let _ = app.emit("meeting-log", &format!("Meeting started: {}", session_id));
            Ok(session_id)
        }
        Err(e) => {
            error!("[MEETING] Failed to start meeting: {}", e);
            let _ = app.emit(
                "meeting-log",
                &format!("ERROR: Failed to start meeting: {}", e),
            );
            Err(e.to_string())
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn stop_meeting(
    app: AppHandle,
    manager: State<'_, Arc<MeetingSessionManager>>,
) -> Result<MeetingSessionSummary, String> {
    info!("[MEETING] stop_meeting called");
    match manager.stop_meeting() {
        Ok(summary) => {
            info!(
                "[MEETING] Meeting stopped: {} ({}s, {} utterances)",
                summary.id, summary.duration_secs, summary.utterance_count
            );
            let _ = app.emit(
                "meeting-log",
                &format!("Meeting stopped: {} utterances", summary.utterance_count),
            );
            Ok(summary)
        }
        Err(e) => {
            error!("[MEETING] Failed to stop meeting: {}", e);
            Err(e.to_string())
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn is_meeting_active(manager: State<'_, Arc<MeetingSessionManager>>) -> bool {
    let active = manager.is_meeting_active();
    info!("[MEETING] is_meeting_active: {}", active);
    active
}

#[tauri::command]
#[specta::specta]
pub fn get_current_session_id(manager: State<'_, Arc<MeetingSessionManager>>) -> Option<String> {
    let session_id = manager.get_current_session_id();
    info!("[MEETING] get_current_session_id: {:?}", session_id);
    session_id
}

#[tauri::command]
#[specta::specta]
pub fn list_meetings(
    manager: State<'_, Arc<MeetingSessionManager>>,
) -> Result<Vec<MeetingSessionSummary>, String> {
    info!("[MEETING] list_meetings called");
    manager.list_meetings().map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn get_meeting_transcript(
    manager: State<'_, Arc<MeetingSessionManager>>,
    session_id: String,
) -> Result<Vec<Utterance>, String> {
    info!("[MEETING] get_meeting_transcript: {}", session_id);
    manager
        .get_meeting_transcript(&session_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn generate_meeting_notes(
    app: AppHandle,
    session_id: String,
    provider_id: String,
    model: String,
    api_key: String,
    system_prompt: String,
    template: String,
) -> Result<(), String> {
    info!(
        "[MEETING] generate_meeting_notes called for session: {}",
        session_id
    );
    let manager = app.state::<Arc<MeetingSessionManager>>();
    let utterances = manager
        .get_meeting_transcript(&session_id)
        .map_err(|e: anyhow::Error| e.to_string())?;

    info!("[MEETING] Transcript has {} utterances", utterances.len());

    if utterances.is_empty() {
        return Err("No transcript available for this meeting".to_string());
    }

    let formatted_transcript: String = utterances
        .iter()
        .map(|u| {
            let speaker = match u.speaker {
                crate::meeting_session::Speaker::You => "You",
                crate::meeting_session::Speaker::Them => "Other",
            };
            format!("[{}] {}", speaker, u.text)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let settings = get_settings(&app);
    let provider = settings
        .post_process_providers
        .iter()
        .find(|p| p.id == provider_id)
        .cloned()
        .ok_or_else(|| format!("Provider '{}' not found", provider_id))?;

    let user_content = format!(
        "{}\n\n---\n\nMeeting Transcript:\n{}",
        template, formatted_transcript
    );

    {
        let mut flag = NOTES_CANCEL_FLAG.lock().await;
        *flag = false;
    }

    info!(
        "[MEETING] Starting notes generation with provider: {}",
        provider_id
    );
    let app_clone = app.clone();
    llm_client::stream_chat_completion(
        &provider,
        api_key,
        &model,
        Some(system_prompt),
        user_content,
        move |token| {
            let _ = app_clone.emit("notes-chunk", &token);
        },
    )
    .await
    .map_err(|e| {
        error!("[MEETING] Notes generation failed: {}", e);
        e.to_string()
    })?;

    info!(
        "[MEETING] Meeting notes generation completed for session: {}",
        session_id
    );
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_notes_generation() -> Result<(), String> {
    info!("[MEETING] cancel_notes_generation called");
    let mut flag = NOTES_CANCEL_FLAG.lock().await;
    *flag = true;
    Ok(())
}
