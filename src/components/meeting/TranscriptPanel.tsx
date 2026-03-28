import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { User, Bot } from "lucide-react";
import { useSessionStore } from "@/stores/sessionStore";
import type { SessionRecord } from "@/bindings";

interface TranscriptPanelProps {
  sessionId?: string | null;
}

export const TranscriptPanel: React.FC<TranscriptPanelProps> = ({
  sessionId,
}) => {
  const { t } = useTranslation();
  const { isRecording, loadTranscript } = useSessionStore();
  const [transcript, setTranscript] = useState<SessionRecord[]>([]);
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [transcript]);

  useEffect(() => {
    if (sessionId) {
      loadTranscript(sessionId).then(setTranscript);
    } else {
      setTranscript([]);
    }
  }, [sessionId, loadTranscript]);

  const displayUtterances = transcript;

  if (displayUtterances.length === 0) {
    return (
      <div className="flex-1 flex items-center justify-center text-mid-gray text-sm">
        {isRecording
          ? t("meeting.transcript.waiting")
          : t("meeting.transcript.empty")}
      </div>
    );
  }

  const formatTime = (ms: number) => {
    const seconds = Math.floor(ms / 1000);
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, "0")}`;
  };

  return (
    <div className="flex-1 overflow-y-auto p-4 space-y-3">
      {displayUtterances.map((record, i) => {
        const isYou = record.speaker === "you";
        const displayText = record.refinedText || record.text;
        return (
          <div
            key={`${record.timestamp}-${i}`}
            className={`flex gap-3 ${isYou ? "flex-row-reverse" : ""}`}
          >
            <div
              className={`shrink-0 w-8 h-8 rounded-full flex items-center justify-center ${
                isYou
                  ? "bg-logo-primary/20 text-logo-primary"
                  : "bg-mid-gray/20 text-mid-gray"
              }`}
            >
              {isYou ? <User size={16} /> : <Bot size={16} />}
            </div>
            <div
              className={`max-w-[80%] rounded-lg p-3 ${
                isYou ? "bg-logo-primary/10" : "bg-mid-gray/10"
              }`}
            >
              <div className="text-xs text-mid-gray mb-1">
                {isYou ? t("meeting.you") : t("meeting.them")} ·{" "}
                {formatTime(record.timestamp)}
              </div>
              <p className="text-sm">{displayText}</p>
            </div>
          </div>
        );
      })}
      <div ref={bottomRef} />
    </div>
  );
};
