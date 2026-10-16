import { tr } from "../../lib/i18n";
import { useEffect, useRef, useState } from "react";

export type AskRequest = {
  title: string;
  body?: string;
  confirmLabel: string;
  cancelLabel?: string;
  danger?: boolean;
  input?: { label: string; value: string; placeholder?: string };
  choices?: { value: string; label: string }[];
  value?: string;
  resolve: (value: string | null) => void;
};

export function AskDialog({
  request,
  onClose,
}: {
  request: AskRequest;
  onClose: () => void;
}) {
  const [value, setValue] = useState(
    request.input?.value ?? request.value ?? request.choices?.[0]?.value ?? "",
  );
  const field = useRef<HTMLInputElement>(null);
  const latest = useRef({ request, onClose });
  latest.current = { request, onClose };
  useEffect(() => {
    field.current?.select();
  }, [request]);
  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        latest.current.request.resolve(null);
        latest.current.onClose();
      }
    };
    document.addEventListener("keydown", escape);
    return () => document.removeEventListener("keydown", escape);
  }, []);
  const done = (accepted: boolean) => {
    request.resolve(
      accepted
        ? request.choices
          ? value
          : request.input
            ? value.trim()
            : "yes"
        : null,
    );
    onClose();
  };
  return (
    <div
      className="modal-backdrop ask-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) done(false);
      }}
    >
      <form
        className="ask-dialog"
        role="alertdialog"
        aria-modal="true"
        onSubmit={(event) => {
          event.preventDefault();
          done(true);
        }}
      >
        <h2>{request.title}</h2>
        {request.body && <p className="modal-copy">{request.body}</p>}
        {request.choices && (
          <div className="ask-choices">
            {request.choices.map((choice) => (
              <label
                className={`ask-choice ${value === choice.value ? "on" : ""}`}
                key={choice.value}
              >
                <input
                  type="radio"
                  name="ask-choice"
                  checked={value === choice.value}
                  onChange={() => setValue(choice.value)}
                />
                {choice.label}
              </label>
            ))}
          </div>
        )}
        {request.input && (
          <label className="field ask-field">
            <span>{request.input.label}</span>
            <input
              ref={field}
              autoFocus
              value={value}
              placeholder={request.input.placeholder}
              onChange={(event) => setValue(event.target.value)}
            />
          </label>
        )}
        <div className="modal-actions">
          <button
            type="button"
            className="button ghost"
            onClick={() => done(false)}
          >
            {request.cancelLabel ?? tr("Cancel")}
          </button>
          <button
            type="submit"
            className={`button ${request.danger ? "danger" : "primary"}`}
            disabled={Boolean(request.input) && !value.trim()}
          >
            {request.confirmLabel}
          </button>
        </div>
      </form>
    </div>
  );
}

