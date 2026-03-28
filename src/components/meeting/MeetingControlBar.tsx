import React from "react";
import { useTranslation } from "react-i18next";
import { Mic, MicOff, Square, Trash2 } from "lucide-react";
import { useSessionStore } from "@/stores/sessionStore";

export const MeetingControlBar: React.FC = () => {
  const { t } = useTranslation();
  const {
    isRecording,
    utteranceCount,
    startSession,
    stopSession,
    discardSession,
  } = useSessionStore();

  const handleToggle = () => {
    if (isRecording) {
      stopSession();
    } else {
      startSession();
    }
  };

  return (
    <div className="w-full max-w-md mx-auto p-4 bg-mid-gray/10 rounded-xl border border-mid-gray/20">
      <div className="flex items-center justify-between mb-4">
        <h2 className="text-lg font-semibold">{t("meeting.title")}</h2>
        <div className="flex items-center gap-2">
          {isRecording && (
            <button
              onClick={discardSession}
              className="flex items-center gap-1.5 px-3 py-2 rounded-lg text-sm text-mid-gray hover:bg-red-500/10 hover:text-red-400 transition-colors"
              title="Discard session"
            >
              <Trash2 size={14} />
            </button>
          )}
          <button
            onClick={handleToggle}
            className={`flex items-center gap-2 px-4 py-2 rounded-lg font-medium transition-all ${
              isRecording
                ? "bg-red-500/20 text-red-400 hover:bg-red-500/30"
                : "bg-logo-primary/80 text-white hover:bg-logo-primary"
            }`}
          >
            {isRecording ? (
              <>
                <Square size={16} />
                {t("meeting.stop")}
              </>
            ) : (
              <>
                <Mic size={16} />
                {t("meeting.start")}
              </>
            )}
          </button>
        </div>
      </div>

      {isRecording && (
        <div className="space-y-3">
          <div className="flex items-center gap-2 text-xs text-mid-gray">
            <span className="w-2 h-2 bg-red-500 rounded-full animate-pulse" />
            {t("meeting.recording")}
            {utteranceCount > 0 && (
              <span className="ml-auto">{utteranceCount} utterances</span>
            )}
          </div>
        </div>
      )}
    </div>
  );
};
