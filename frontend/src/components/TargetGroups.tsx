import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { errorText } from "../api";
import type { Target, TargetGroup } from "../types";
import { Dialog } from "./ui";

function targetsWord(count: number): string {
  const mod100 = count % 100;
  const mod10 = count % 10;
  if (mod100 >= 11 && mod100 <= 14) return "целей";
  if (mod10 === 1) return "цель";
  if (mod10 >= 2 && mod10 <= 4) return "цели";
  return "целей";
}

function rulesWord(count: number): string {
  const mod100 = count % 100;
  const mod10 = count % 10;
  if (mod100 >= 11 && mod100 <= 14) return "правил";
  if (mod10 === 1) return "правило";
  if (mod10 >= 2 && mod10 <= 4) return "правила";
  return "правил";
}

export function TargetGroupsDialog({
  groups,
  targets,
  onClose,
  onChanged,
  onError,
  error,
}: {
  groups: TargetGroup[];
  targets: Target[];
  onClose: () => void;
  onChanged: () => Promise<void>;
  onError: (message: string) => void;
  error?: string | null;
}) {
  const [name, setName] = useState("");
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<TargetGroup | null>(null);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const confirmCancelRef = useRef<HTMLButtonElement>(null);
  const confirmingTriggerRef = useRef<HTMLButtonElement | null>(null);
  const wasConfirmingRef = useRef(false);
  useEffect(() => {
    if (confirming) confirmCancelRef.current?.focus();
    else if (wasConfirmingRef.current && confirmingTriggerRef.current?.isConnected) {
      confirmingTriggerRef.current.focus();
    }
    wasConfirmingRef.current = confirming !== null;
  }, [confirming]);

  const create = async () => {
    if (name.trim() === "") return;
    setBusy("new");
    try {
      await api.createTargetGroup(name.trim());
      setName("");
      await onChanged();
    } catch (cause) {
      onError(errorText(cause));
    } finally {
      setBusy(null);
    }
  };

  const rename = async (id: string) => {
    if (renameValue.trim() === "") return;
    setBusy(id);
    try {
      await api.renameTargetGroup(id, renameValue.trim());
      setRenaming(null);
      await onChanged();
    } catch (cause) {
      onError(errorText(cause));
    } finally {
      setBusy(null);
    }
  };

  const remove = async (group: TargetGroup) => {
    setBusy(group.id);
    try {
      await api.deleteTargetGroup(group.id);
      setConfirming(null);
      await onChanged();
      closeButtonRef.current?.focus();
    } catch (cause) {
      onError(errorText(cause));
    } finally {
      setBusy(null);
    }
  };

  const confirmingTargetCount = confirming
    ? targets.filter((target) => target.group?.id === confirming.id).length
    : 0;

  return (
    <Dialog
      title="Группы целей"
      subtitle="Один уровень вложенности. Группа не меняет проверки и не уходит агенту."
      onClose={onClose}
      wide
      footer={
        <button ref={closeButtonRef} type="button" className="btn btn--ghost" onClick={onClose} disabled={busy !== null}>
          Закрыть
        </button>
      }
    >
      <div
        className="target-groups-dialog"
        role="group"
        aria-label="Управление группами целей"
        onKeyDown={(event) => {
          if (event.key === "Escape" && confirming) {
            event.preventDefault();
            event.stopPropagation();
            setConfirming(null);
          }
        }}
      >
      <div className="group-create">
        <input
          value={name}
          maxLength={64}
          placeholder="Название новой группы"
          aria-label="Название новой группы"
          data-dialog-autofocus
          onChange={(event) => setName(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") void create();
          }}
        />
        <button
          type="button"
          className="btn btn--small"
          disabled={busy !== null || name.trim() === ""}
          onClick={() => void create()}
        >
          Создать
        </button>
      </div>
      {error ? <p className="form-error">{error}</p> : null}
      {confirming ? (
        <div
          className="confirm-panel"
          role="group"
          aria-live="polite"
          aria-labelledby="group-delete-confirm-title"
          aria-describedby="group-delete-confirm-text"
        >
          <p id="group-delete-confirm-title" className="confirm-panel__title">Удалить группу «{confirming.name}»?</p>
          <p id="group-delete-confirm-text">
            {confirmingTargetCount > 0
              ? `Цели (${confirmingTargetCount}) останутся без группы. Действие нельзя отменить.`
              : "Действие нельзя отменить."}
          </p>
          <div className="confirm-panel__actions">
            <button
              ref={confirmCancelRef}
              type="button"
              className="btn btn--ghost"
              onClick={() => setConfirming(null)}
              disabled={busy !== null}
            >
              Отмена
            </button>
            <button
              type="button"
              className="btn btn--danger"
              onClick={() => void remove(confirming)}
              disabled={busy !== null}
            >
              {busy === confirming.id ? "Удаляем…" : "Удалить группу"}
            </button>
          </div>
        </div>
      ) : null}
      {groups.length === 0 ? (
        <p className="muted">Групп пока нет. Создайте первую — и привяжите к ней цели.</p>
      ) : (
        <ul className="group-list">
          {groups.map((group) => {
            const count = targets.filter((target) => target.group?.id === group.id).length;
            return (
              <li key={group.id} className="group-list__item">
                {renaming === group.id ? (
                  <input
                    autoFocus
                    aria-label={`Новое имя группы ${group.name}`}
                    value={renameValue}
                    maxLength={64}
                    onChange={(event) => setRenameValue(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") void rename(group.id);
                      if (event.key === "Escape") setRenaming(null);
                    }}
                  />
                ) : (
                  <span className="group-list__name">
                    <strong>{group.name}</strong>
                    <small>
                      {count} {targetsWord(count)}
                      {group.rule_count > 0
                        ? ` · ${group.rule_count} ${rulesWord(group.rule_count)}`
                        : ""}
                    </small>
                  </span>
                )}
                <span className="group-list__actions">
                  {renaming === group.id ? (
                    <>
                      <button
                        type="button"
                        className="btn btn--small"
                        disabled={busy !== null || renameValue.trim() === ""}
                        onClick={() => void rename(group.id)}
                      >
                        Сохранить
                      </button>
                      <button
                        type="button"
                        className="btn btn--ghost btn--small"
                        onClick={() => setRenaming(null)}
                      >
                        Отмена
                      </button>
                    </>
                  ) : (
                    <>
                      <button
                        type="button"
                        className="btn btn--ghost btn--small"
                        aria-label={`Переименовать группу ${group.name}`}
                        onClick={() => {
                          setRenaming(group.id);
                          setRenameValue(group.name);
                        }}
                      >
                        Переименовать
                      </button>
                      <button
                        type="button"
                        className="btn btn--ghost btn--small"
                        aria-label={`Удалить группу ${group.name}`}
                        disabled={busy !== null}
                        onClick={(event) => {
                          confirmingTriggerRef.current = event.currentTarget;
                          setConfirming(group);
                        }}
                      >
                        Удалить
                      </button>
                    </>
                  )}
                </span>
              </li>
            );
          })}
        </ul>
      )}
      </div>
    </Dialog>
  );
}
