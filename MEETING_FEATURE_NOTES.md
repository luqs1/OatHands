# OpenOats Desktop - Meeting Feature Notes

## Goal
Build a Windows `.exe` of Handy with a meeting notes feature:
- Mic + system audio capture (dual-stream)
- Real-time transcription
- AI-powered notes generation

## Current Status

### Completed
- ✅ Basic meeting feature code (frontend + backend)
- ⚠️ Build blocked - missing Tauri plugins in Cargo.toml

### What's Been Done

#### Backend (Rust)
1. **`src-tauri/src/meeting_session.rs`** - Meeting session management
   - Session state, audio stream handling, transcription coordination

2. **`src-tauri/src/commands/meeting.rs`** - Tauri commands
   - `start_meeting`, `stop_meeting`, `get_meeting_status`, etc.

3. **`src-tauri/src/audio_toolkit/system_capture.rs`** - System audio capture
   - Was designed for Linux pulseaudio (incomplete for Windows)

4. **`src-tauri/src/llm_client.rs`** - LLM client updates
   - Added streaming support for AI notes generation

#### Frontend (React/TypeScript)
1. **`src/components/meeting/`** - Meeting UI components
   - `MeetingView.tsx` - Main meeting view
   - `TranscriptPanel.tsx` - Live transcript display
   - `NotesPanel.tsx` - Generated notes display
   - `MeetingControlBar.tsx` - Start/stop controls

2. **`src/stores/meetingStore.ts`** - Zustand store
   - Meeting state management, WebSocket connection

3. **Modified files:**
   - `src/components/Sidebar.tsx` - Added Meeting section
   - `src/main.tsx` - Store initialization
   - `src/bindings.ts` - Meeting types
   - `src/i18n/locales/en/translation.json` - i18n keys

## Build Errors

### Current Blocker
```
Permission autostart:default not found
Permission global-shortcut:allow-is-registered not found
```

### Root Cause
The `Cargo.toml` is missing Tauri plugin dependencies that the code uses:
- `tauri-plugin-autostart`
- `tauri-plugin-global-shortcut`
- `tauri-plugin-updater`
- `handy-keys`

### Fix Required
Add these to `src-tauri/Cargo.toml`:

```toml
[target.'cfg(not(any(target_os = "android", target_os = "ios")))'.dependencies]
tauri-plugin-autostart = "2.5.1"
tauri-plugin-global-shortcut = "2.3.1"
tauri-plugin-updater = "2.10.0"
tauri-plugin-single-instance = "2.3.2"

# Also in dependencies section:
handy-keys = "0.2.4"
```

## What's Still Needed

### 1. Fix Build (Critical)
- Add missing dependencies to Cargo.toml
- May need to also add permissions to `src-tauri/capabilities/desktop.json`

### 2. Windows Audio Capture
- `system_capture.rs` uses Linux pulseaudio - needs Windows implementation
- Windows alternatives: Wasapi, DirectSound, or Core Audio API

### 3. Meeting Feature Incomplete
- Basic UI exists but not fully connected to backend
- Audio capture not implemented for Windows
- Transcription pipeline not fully wired up

### 4. transcribe-rs Configuration
Current config for Windows:
```toml
transcribe-rs = { version = "0.3.2", features = ["whisper-cpp", "onnx"] }
whisper-rs = { version = "0.16.0" }
whisper-rs-sys = { version = "0.15.0" }
```

## Files to Check
- `src-tauri/capabilities/desktop.json` - Permissions
- `src-tauri/capabilities/default.json` - Permissions
- `src-tauri/src/lib.rs` - Plugin initialization
- `src-tauri/src/shortcut/mod.rs` - Uses autostart plugin
