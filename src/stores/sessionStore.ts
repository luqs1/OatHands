/**
 * Session store — uses the new session coordinator commands.
 * Replaces meetingStore.ts for the meeting lifecycle.
 * Swift: AppCoordinator + LiveSessionController (state projection)
 */
import { create } from "zustand";
import { subscribeWithSelector } from "zustand/middleware";
import { listen } from "@tauri-apps/api/event";
import {
  commands,
  type SessionIndex,
  type SessionRecord,
  type LiveSessionState,
  type MeetingTemplate,
} from "@/bindings";

interface SessionStore {
  // Live session state
  isRecording: boolean;
  sessionId: string | null;
  utteranceCount: number;
  volatileYouText: string;
  volatileThemText: string;

  // Session history
  sessions: SessionIndex[];
  selectedSessionId: string | null;

  // Notes
  isGeneratingNotes: boolean;
  notes: string;
  notesError: string | null;

  // Templates
  templates: MeetingTemplate[];
  selectedTemplateId: string | null;

  // Lifecycle
  initialized: boolean;
  initialize: () => Promise<void>;

  // Actions
  startSession: () => Promise<boolean>;
  stopSession: () => Promise<void>;
  discardSession: () => Promise<void>;
  loadSessions: () => Promise<void>;
  selectSession: (sessionId: string | null) => void;
  loadTranscript: (sessionId: string) => Promise<SessionRecord[]>;
  deleteSession: (sessionId: string) => Promise<void>;
  renameSession: (sessionId: string, title: string) => Promise<void>;
  generateNotes: (sessionId: string) => Promise<void>;
  loadTemplates: () => Promise<void>;
  selectTemplate: (templateId: string) => void;
  clearNotes: () => void;
  pollLiveState: () => Promise<void>;
}

export const useSessionStore = create<SessionStore>()(
  subscribeWithSelector((set, get) => ({
    isRecording: false,
    sessionId: null,
    utteranceCount: 0,
    volatileYouText: "",
    volatileThemText: "",
    sessions: [],
    selectedSessionId: null,
    isGeneratingNotes: false,
    notes: "",
    notesError: null,
    templates: [],
    selectedTemplateId: null,
    initialized: false,

    initialize: async () => {
      if (get().initialized) return;

      // Check if a session is already active
      try {
        const result = await commands.sessionLiveState();
        if (result.status === "ok") {
          const state = result.data;
          set({
            isRecording: state.isRecording,
            sessionId: state.sessionId,
            utteranceCount: state.utteranceCount,
          });
        }
      } catch (e) {
        console.warn("Failed to check session status:", e);
      }

      // Load history and templates
      await Promise.all([get().loadSessions(), get().loadTemplates()]);

      // Listen for session events from the coordinator
      listen<string>("session-started", (event) => {
        set({
          isRecording: true,
          sessionId: event.payload,
          utteranceCount: 0,
          notes: "",
          notesError: null,
        });
      });

      listen("session-stopped", () => {
        set({ isRecording: false, sessionId: null });
        get().loadSessions();
      });

      listen("session-discarded", () => {
        set({ isRecording: false, sessionId: null, utteranceCount: 0 });
      });

      // Notes streaming
      listen<string>("session-notes-chunk", (event) => {
        set((state) => ({ notes: state.notes + event.payload }));
      });

      listen<string>("session-notes-complete", () => {
        set({ isGeneratingNotes: false });
        get().loadSessions(); // refresh hasNotes
      });

      listen<string>("session-notes-error", (event) => {
        set({ notesError: event.payload, isGeneratingNotes: false });
      });

      set({ initialized: true });
    },

    // Swift: LiveSessionController.startSession(settings:)
    startSession: async () => {
      try {
        const result = await commands.sessionStart();
        if (result.status === "ok") {
          set({
            isRecording: true,
            sessionId: result.data,
            utteranceCount: 0,
            notes: "",
            notesError: null,
          });
          return true;
        }
        console.error("Failed to start session:", result.error);
        return false;
      } catch (err) {
        console.error("Failed to start session:", err);
        return false;
      }
    },

    // Swift: LiveSessionController.stopSession(settings:)
    stopSession: async () => {
      try {
        const result = await commands.sessionStop();
        if (result.status === "ok") {
          set({ isRecording: false, sessionId: null });
          await get().loadSessions();
        } else {
          console.error("Failed to stop session:", result.error);
        }
      } catch (err) {
        console.error("Failed to stop session:", err);
      }
    },

    discardSession: async () => {
      try {
        await commands.sessionDiscard();
        set({ isRecording: false, sessionId: null, utteranceCount: 0 });
      } catch (err) {
        console.error("Failed to discard session:", err);
      }
    },

    // Swift: AppCoordinator.loadHistory()
    loadSessions: async () => {
      try {
        const result = await commands.sessionList();
        if (result.status === "ok") {
          set({ sessions: result.data });
        }
      } catch (err) {
        console.error("Failed to load sessions:", err);
      }
    },

    selectSession: (sessionId) => set({ selectedSessionId: sessionId }),

    loadTranscript: async (sessionId) => {
      try {
        const result = await commands.sessionTranscript(sessionId);
        if (result.status === "ok") {
          return result.data;
        }
        return [];
      } catch (err) {
        console.error("Failed to load transcript:", err);
        return [];
      }
    },

    deleteSession: async (sessionId) => {
      try {
        await commands.sessionDelete(sessionId);
        set((state) => ({
          sessions: state.sessions.filter((s) => s.id !== sessionId),
          selectedSessionId:
            state.selectedSessionId === sessionId
              ? null
              : state.selectedSessionId,
        }));
      } catch (err) {
        console.error("Failed to delete session:", err);
      }
    },

    renameSession: async (sessionId, title) => {
      try {
        await commands.sessionRename(sessionId, title);
        await get().loadSessions();
      } catch (err) {
        console.error("Failed to rename session:", err);
      }
    },

    // Swift: NotesController.generateNotes(for:template:)
    generateNotes: async (sessionId) => {
      set({ isGeneratingNotes: true, notes: "", notesError: null });

      const templateId = get().selectedTemplateId;
      try {
        const result = await commands.sessionGenerateNotes(
          sessionId,
          templateId
        );
        if (result.status === "error") {
          set({ notesError: result.error, isGeneratingNotes: false });
        }
        // On success, the notes arrive via session-notes-chunk events
      } catch (err) {
        set({ notesError: String(err), isGeneratingNotes: false });
      }
    },

    loadTemplates: async () => {
      try {
        const result = await commands.sessionTemplates();
        if (result.status === "ok") {
          set({ templates: result.data });
          // Default to Generic template
          if (!get().selectedTemplateId && result.data.length > 0) {
            set({ selectedTemplateId: result.data[0].id });
          }
        }
      } catch (err) {
        console.error("Failed to load templates:", err);
      }
    },

    selectTemplate: (templateId) => set({ selectedTemplateId: templateId }),

    clearNotes: () => set({ notes: "", notesError: null }),

    pollLiveState: async () => {
      try {
        const result = await commands.sessionLiveState();
        if (result.status === "ok") {
          set({
            isRecording: result.data.isRecording,
            sessionId: result.data.sessionId,
            utteranceCount: result.data.utteranceCount,
            volatileYouText: result.data.volatileYouText,
            volatileThemText: result.data.volatileThemText,
          });
        }
      } catch {
        // Ignore polling errors
      }
    },
  }))
);
