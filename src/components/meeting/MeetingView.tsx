import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Calendar, Clock, MessageSquare, ChevronRight, FileText, Trash2, Tag } from "lucide-react";
import { useSessionStore } from "@/stores/sessionStore";
import { MeetingControlBar } from "./MeetingControlBar";
import { TranscriptPanel } from "./TranscriptPanel";
import { NotesPanel } from "./NotesPanel";

type Tab = "transcript" | "notes";

export const MeetingView: React.FC = () => {
  const { t } = useTranslation();
  const {
    isRecording,
    sessionId,
    sessions,
    selectedSessionId,
    selectSession,
    deleteSession,
    initialize,
  } = useSessionStore();
  const [activeTab, setActiveTab] = useState<Tab>("transcript");

  useEffect(() => {
    initialize();
  }, [initialize]);

  const displaySessionId = isRecording ? sessionId : selectedSessionId;

  const formatDate = (timestamp: number) => {
    return new Date(timestamp).toLocaleDateString(undefined, {
      month: "short",
      day: "numeric",
      year: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  };

  const formatDuration = (startMs: number, endMs: number | null) => {
    if (!endMs) return "ongoing";
    const secs = Math.floor((endMs - startMs) / 1000);
    const mins = Math.floor(secs / 60);
    return `${mins} min`;
  };

  return (
    <div className="w-full max-w-3xl mx-auto p-6 space-y-6">
      <MeetingControlBar />

      {!isRecording && sessions.length > 0 && (
        <div className="bg-mid-gray/5 rounded-xl border border-mid-gray/20 p-4">
          <h3 className="text-sm font-medium mb-3 flex items-center gap-2">
            <Calendar size={14} />
            {t("meeting.history.title")}
          </h3>
          <div className="space-y-2 max-h-64 overflow-y-auto">
            {sessions.slice(0, 20).map((session) => (
              <button
                key={session.id}
                onClick={() => selectSession(session.id)}
                className={`w-full text-left p-3 rounded-lg border transition-colors group ${
                  selectedSessionId === session.id
                    ? "border-logo-primary/50 bg-logo-primary/5"
                    : "border-transparent hover:bg-mid-gray/10"
                }`}
              >
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-3 flex-1 min-w-0">
                    <Clock size={14} className="text-mid-gray shrink-0" />
                    <div className="min-w-0">
                      <div className="text-sm font-medium truncate">
                        {session.title || formatDate(session.startedAt)}
                      </div>
                      <div className="text-xs text-mid-gray flex items-center gap-2">
                        {!session.title && <span>{formatDuration(session.startedAt, session.endedAt)}</span>}
                        {session.title && <span>{formatDate(session.startedAt)}</span>}
                        <span>·</span>
                        <span>{session.utteranceCount} utterances</span>
                        {session.hasNotes && (
                          <>
                            <span>·</span>
                            <FileText size={10} className="text-logo-primary" />
                          </>
                        )}
                        {session.meetingApp && (
                          <>
                            <span>·</span>
                            <span>{session.meetingApp}</span>
                          </>
                        )}
                      </div>
                    </div>
                  </div>
                  <div className="flex items-center gap-1">
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        deleteSession(session.id);
                      }}
                      className="p-1 rounded opacity-0 group-hover:opacity-100 hover:bg-red-500/10 hover:text-red-400 transition-all"
                    >
                      <Trash2 size={12} />
                    </button>
                    <ChevronRight size={14} className="text-mid-gray" />
                  </div>
                </div>
              </button>
            ))}
          </div>
        </div>
      )}

      {(isRecording || displaySessionId) && (
        <div className="bg-mid-gray/5 rounded-xl border border-mid-gray/20 overflow-hidden">
          <div className="flex border-b border-mid-gray/20">
            <button
              onClick={() => setActiveTab("transcript")}
              className={`flex-1 px-4 py-3 text-sm font-medium flex items-center justify-center gap-2 transition-colors ${
                activeTab === "transcript"
                  ? "border-b-2 border-logo-primary text-logo-primary"
                  : "text-mid-gray hover:text-foreground"
              }`}
            >
              <MessageSquare size={14} />
              {t("meeting.tabs.transcript")}
            </button>
            <button
              onClick={() => setActiveTab("notes")}
              className={`flex-1 px-4 py-3 text-sm font-medium flex items-center justify-center gap-2 transition-colors ${
                activeTab === "notes"
                  ? "border-b-2 border-logo-primary text-logo-primary"
                  : "text-mid-gray hover:text-foreground"
              }`}
            >
              {t("meeting.tabs.notes")}
            </button>
          </div>

          {activeTab === "transcript" ? (
            <TranscriptPanel sessionId={displaySessionId} />
          ) : (
            <NotesPanel sessionId={displaySessionId} />
          )}
        </div>
      )}
    </div>
  );
};
