import React from "react";
import { useTranslation } from "react-i18next";
import { FileText, Copy, Loader2, AlertCircle } from "lucide-react";
import { useSessionStore } from "@/stores/sessionStore";

interface NotesPanelProps {
  sessionId?: string | null;
}

export const NotesPanel: React.FC<NotesPanelProps> = ({ sessionId }) => {
  const { t } = useTranslation();
  const {
    notes,
    notesError,
    isGeneratingNotes,
    generateNotes,
    clearNotes,
    templates,
    selectedTemplateId,
    selectTemplate,
  } = useSessionStore();

  const handleGenerateNotes = () => {
    if (sessionId) {
      clearNotes();
      generateNotes(sessionId);
    }
  };

  const handleCopy = () => {
    navigator.clipboard.writeText(notes);
  };

  return (
    <div className="flex-1 flex flex-col p-4">
      <div className="flex items-center justify-between mb-4">
        <h3 className="text-sm font-medium flex items-center gap-2">
          <FileText size={16} />
          {t("meeting.notes.title")}
        </h3>
        {sessionId && (
          <div className="flex items-center gap-2">
            {templates.length > 0 && (
              <select
                value={selectedTemplateId ?? ""}
                onChange={(e) => selectTemplate(e.target.value)}
                className="text-xs px-2 py-1.5 rounded-lg border border-mid-gray/30 bg-transparent"
              >
                {templates.map((t) => (
                  <option key={t.id} value={t.id}>
                    {t.name}
                  </option>
                ))}
              </select>
            )}
            <button
              onClick={handleGenerateNotes}
              disabled={isGeneratingNotes}
              className="text-xs px-3 py-1.5 rounded-lg bg-logo-primary/80 text-white hover:bg-logo-primary disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-1.5"
            >
              {isGeneratingNotes ? (
                <>
                  <Loader2 size={12} className="animate-spin" />
                  {t("meeting.notes.generating")}
                </>
              ) : notes ? (
                t("meeting.notes.regenerate")
              ) : (
                t("meeting.notes.generate")
              )}
            </button>
          </div>
        )}
      </div>

      {notesError && (
        <div className="mb-3 p-3 bg-red-500/10 border border-red-500/20 rounded-lg text-sm text-red-400 flex items-start gap-2">
          <AlertCircle size={16} className="shrink-0 mt-0.5" />
          {notesError}
        </div>
      )}

      <div className="flex-1 bg-mid-gray/5 rounded-lg border border-mid-gray/20 p-3 overflow-y-auto">
        {notes ? (
          <div className="prose prose-sm max-w-none">
            <pre className="whitespace-pre-wrap text-sm font-sans">
              {notes}
            </pre>
          </div>
        ) : isGeneratingNotes ? (
          <div className="flex items-center justify-center h-full text-mid-gray text-sm">
            <Loader2 size={20} className="animate-spin mr-2" />
            {t("meeting.notes.generating")}
          </div>
        ) : (
          <div className="flex flex-col items-center justify-center h-full text-mid-gray text-sm">
            <FileText size={32} className="mb-2 opacity-50" />
            {t("meeting.notes.empty")}
          </div>
        )}
      </div>

      {notes && (
        <div className="mt-3 flex justify-end">
          <button
            onClick={handleCopy}
            className="text-xs px-3 py-1.5 rounded-lg border border-mid-gray/30 hover:bg-mid-gray/10 flex items-center gap-1.5"
          >
            <Copy size={12} />
            {t("meeting.notes.copy")}
          </button>
        </div>
      )}
    </div>
  );
};
