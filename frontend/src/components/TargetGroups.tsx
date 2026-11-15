import { useState } from "react";
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
    const count = targets.filter((target) => target.group?.id === group.id).length;
    const warning =
      count > 0
        ? `Цели (${count}) останутся без группы. Продолжить?`
        : "Удалить группу?";
    if (!window.confirm(warning)) return;
    setBusy(group.id);
    try {
      await api.deleteTargetGroup(group.id);
      await onChanged();
    } catch (cause) {
      onError(errorText(cause));
    } finally {
      setBusy(null);
    }
  };

  return (
    <Dialog
      title="Группы целей"
      subtitle="Один уровень вложенности. Группа не меняет проверки и не уходит агенту."
      onClose={onClose}
      wide
      footer={
        <button type="button" className="btn btn--ghost" onClick={onClose}>
          Закрыть
        </button>
      }
    >
      <div className="group-create">
        <input
          value={name}
          maxLength={64}
          placeholder="Название новой группы"
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
                        disabled={busy !== null}
                        onClick={() => void remove(group)}
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
    </Dialog>
  );
}
