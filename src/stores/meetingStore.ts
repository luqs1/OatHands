import { create } from "zustand";
import { subscribeWithSelector } from "zustand/middleware";
import { produce } from "immer";
import { listen } from "@tauri-apps/api/event";
import {
  commands,
  type Speaker,
  type Utterance,
  type MeetingSessionSummary,
} from "@/bindings";
import { useSettingsStore } from "./settingsStore";

interface MeetingUtterance extends Utterance {
  isFinal: boolean;
}

interface MeetingsStore {
  isActive: boolean;
  currentSessionId: string | null;
  isRecording: boolean;
  micLevel: number;
  systemLevel: number;
  utterances: MeetingUtterance[];
  currentMeeting: MeetingSessionSummary | null;
  pastMeetings: MeetingSessionSummary[];
  isGeneratingNotes: boolean;
  notes: string;
  notesError: string | null;
  initialized: boolean;

  initialize: () => Promise<void>;
  startMeeting: () => Promise<boolean>;
  stopMeeting: () => Promise<MeetingSessionSummary | null>;
  loadPastMeetings: () => Promise<void>;
  loadTranscript: (sessionId: string) => Promise<Utterance[]>;
  generateNotes: (sessionId: string) => Promise<void>;
  cancelNotesGeneration: () => Promise<void>;
  clearNotes: () => void;
  setMicLevel: (level: number) => void;
  setSystemLevel: (level: number) => void;
}

export const useMeetingsStore = create<MeetingsStore>()(
  subscribeWithSelector((set, get) => ({
    isActive: false,
    currentSessionId: null,
    isRecording: false,
    micLevel: 0,
    systemLevel: 0,
    utterances: [],
    currentMeeting: null,
    pastMeetings: [],
    isGeneratingNotes: false,
    notes: "",
    notesError: null,
    initialized: false,

    initialize: async () => {
      if (get().initialized) return;

      try {
        const active = await commands.isMeetingActive();
        if (active) {
          const sessionId = await commands.getCurrentSessionId();
          if (sessionId) {
            set({ isActive: true, currentSessionId: sessionId });
          }
        }
      } catch (e) {
        console.warn("Failed to check meeting status:", e);
      }

      await get().loadPastMeetings();

      listen<{ id: string; speaker: Speaker; text: string; timestamp_ms: number; duration_ms: number; is_final: boolean }>(
        "meeting-utterance",
        (event) => {
          const utt = event.payload;
          set(
            produce((state) => {
              const existing = state.utterances.find((u: MeetingUtterance) => u.id === utt.id);
              if (existing) {
                existing.text = utt.text;
                existing.isFinal = utt.is_final;
              } else {
                state.utterances.push({
                  ...utt,
                  isFinal: utt.is_final,
                });
              }
            }),
          );
        },
      );

      listen<string>("meeting-started", (event) => {
        set({
          isActive: true,
          currentSessionId: event.payload,
          isRecording: true,
          utterances: [],
          notes: "",
          notesError: null,
        });
      });

      listen<MeetingSessionSummary>("meeting-stopped", (event) => {
        set({
          isActive: false,
          currentSessionId: null,
          isRecording: false,
          currentMeeting: event.payload,
        });
        get().loadPastMeetings();
      });

      listen<{ mic_rms: number; system_rms: number }>("meeting-audio-level", (event) => {
        set({
          micLevel: event.payload.mic_rms,
          systemLevel: event.payload.system_rms,
        });
      });

      listen<string>("notes-chunk", (event) => {
        set((state) => ({ notes: state.notes + event.payload }));
      });

      set({ initialized: true });
    },

    startMeeting: async () => {
      try {
        const result = await commands.startMeeting();
        if (result.status === "ok") {
          set({
            isActive: true,
            currentSessionId: result.data,
            isRecording: true,
            utterances: [],
            notes: "",
            notesError: null,
          });
          return true;
        } else {
          console.error("Failed to start meeting:", result.error);
          return false;
        }
      } catch (err) {
        console.error("Failed to start meeting:", err);
        return false;
      }
    },

    stopMeeting: async () => {
      try {
        const result = await commands.stopMeeting();
        if (result.status === "ok") {
          set({
            isActive: false,
            currentSessionId: null,
            isRecording: false,
            currentMeeting: result.data,
          });
          await get().loadPastMeetings();
          return result.data;
        } else {
          console.error("Failed to stop meeting:", result.error);
          return null;
        }
      } catch (err) {
        console.error("Failed to stop meeting:", err);
        return null;
      }
    },

    loadPastMeetings: async () => {
      try {
        const result = await commands.listMeetings();
        if (result.status === "ok") {
          set({ pastMeetings: result.data });
        }
      } catch (err) {
        console.error("Failed to load past meetings:", err);
      }
    },

    loadTranscript: async (sessionId: string) => {
      try {
        const result = await commands.getMeetingTranscript(sessionId);
        if (result.status === "ok") {
          return result.data;
        }
        return [];
      } catch (err) {
        console.error("Failed to load transcript:", err);
        return [];
      }
    },

    generateNotes: async (sessionId: string) => {
      set({ isGeneratingNotes: true, notes: "", notesError: null });

      const settings = useSettingsStore.getState().settings;
      if (!settings) {
        set({ notesError: "Settings not loaded", isGeneratingNotes: false });
        return;
      }

      const providerId = settings.post_process_provider_id || "openrouter";
      const apiKey = settings.post_process_api_keys?.[providerId] || "";
      const model = settings.post_process_models?.[providerId] || "openai/gpt-4o-mini";
      const selectedPromptId = settings.post_process_selected_prompt_id;
      const prompts = settings.post_process_prompts || [];
      const selectedPrompt = prompts.find((p) => p.id === selectedPromptId);
      const systemPrompt = selectedPrompt?.prompt || "You are a professional meeting notes assistant. Based on the transcript, generate concise notes with key points and action items.";

      if (!apiKey) {
        set({ notesError: "API key not configured for the selected provider", isGeneratingNotes: false });
        return;
      }

      const template = `# Meeting Notes

## Key Discussion Points

## Decisions Made

## Action Items

## Questions Raised

## Next Steps
`;

      try {
        const result = await commands.generateMeetingNotes(
          sessionId,
          providerId,
          model,
          apiKey,
          systemPrompt,
          template,
        );

        if (result.status === "error") {
          set({ notesError: result.error, isGeneratingNotes: false });
        } else {
          set({ isGeneratingNotes: false });
        }
      } catch (err) {
        set({ notesError: String(err), isGeneratingNotes: false });
      }
    },

    cancelNotesGeneration: async () => {
      try {
        await commands.cancelNotesGeneration();
        set({ isGeneratingNotes: false });
      } catch (err) {
        console.error("Failed to cancel notes generation:", err);
      }
    },

    clearNotes: () => set({ notes: "", notesError: null }),

    setMicLevel: (level) => set({ micLevel: level }),
    setSystemLevel: (level) => set({ systemLevel: level }),
  })),
);
