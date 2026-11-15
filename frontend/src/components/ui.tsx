import { useEffect, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

interface DialogProps {
  title: string;
  subtitle?: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  wide?: boolean;
}

export function Dialog({ title, subtitle, onClose, children, footer, wide }: DialogProps) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  return createPortal(
    <div
      className="overlay"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        className={wide ? "dialog dialog--wide" : "dialog"}
        role="dialog"
        aria-modal="true"
        aria-labelledby="dialog-title"
      >
        <header className="dialog-head">
          <div>
            <h2 id="dialog-title">{title}</h2>
            {subtitle ? <p className="dialog-sub">{subtitle}</p> : null}
          </div>
          <button type="button" className="icon-button" onClick={onClose} aria-label="Закрыть">
            ×
          </button>
        </header>
        <div className="dialog-body">{children}</div>
        {footer ? <footer className="dialog-actions">{footer}</footer> : null}
      </div>
    </div>,
    document.body,
  );
}

interface CopyButtonProps {
  text: string;
  label?: string;
  className?: string;
}

export function CopyButton({ text, label = "Копировать", className }: CopyButtonProps) {
  const [state, setState] = useState<"idle" | "done" | "failed">("idle");

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setState("done");
    } catch {
      setState("failed");
    }
    window.setTimeout(() => setState("idle"), 1600);
  };

  return (
    <button type="button" className={className ?? "copy"} onClick={copy}>
      {state === "done" ? "Скопировано" : state === "failed" ? "Выделите вручную" : label}
    </button>
  );
}

export function StatusDot({ health }: { health: "ok" | "warn" | "fail" | "idle" | "offline" }) {
  return <i className={`dot dot--${health}`} aria-hidden="true" />;
}
