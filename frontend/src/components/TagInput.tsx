import { useEffect, useMemo, useRef, useState, useId, type KeyboardEvent, type PointerEvent } from "react";
import {
  addTag,
  filterTagSuggestions,
  MAX_TAGS,
  normalizeTag,
  removeTag,
  resolveTagChoice,
} from "../tagInputLogic.mjs";

export { normalizeTag } from "../tagInputLogic.mjs";

interface TagInputProps {
  value: string[];
  onChange: (tags: string[]) => void;
  suggestions?: string[];
  placeholder?: string;
  id?: string;
  ariaLabel?: string;
}

interface TagOption {
  value: string;
  create: boolean;
}

export function TagInput({
  value,
  onChange,
  suggestions = [],
  placeholder = "Найти или добавить тег",
  id,
  ariaLabel,
}: TagInputProps) {
  const generatedId = useId().replaceAll(":", "");
  const inputId = id ?? `tag-input-${generatedId}`;
  const listId = `${inputId}-options`;
  const hintId = `${inputId}-hint`;
  const inputRef = useRef<HTMLInputElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const restoreInputFocusRef = useRef(false);
  const [draft, setDraft] = useState("");
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(-1);
  const [feedback, setFeedback] = useState<string | null>(null);
  const query = normalizeTag(draft);
  const atLimit = value.length >= MAX_TAGS;
  const selected = useMemo(() => new Set(value.map(normalizeTag)), [value]);
  const filtered = useMemo(
    () => filterTagSuggestions(draft, value, suggestions),
    [draft, value, suggestions],
  );
  const canCreate =
    query !== "" &&
    !selected.has(query) &&
    !suggestions.some((tag) => normalizeTag(tag) === query);
  const options = useMemo<TagOption[]>(
    () => [
      ...filtered.map((tag) => ({ value: tag, create: false })),
      ...(canCreate ? [{ value: query, create: true }] : []),
    ],
    [canCreate, filtered, query],
  );
  const activeOption = activeIndex >= 0 ? options[activeIndex] : undefined;

  useEffect(() => {
    if (!restoreInputFocusRef.current) return;
    restoreInputFocusRef.current = false;
    inputRef.current?.focus({ preventScroll: true });
  }, [value]);

  useEffect(() => {
    if (!open || activeIndex < 0) return;
    const menu = menuRef.current;
    const option = menu?.children.item(activeIndex) as HTMLElement | null;
    if (!menu || !option) return;
    const menuBounds = menu.getBoundingClientRect();
    const optionBounds = option.getBoundingClientRect();
    if (optionBounds.bottom > menuBounds.bottom) {
      menu.scrollTop += optionBounds.bottom - menuBounds.bottom;
    } else if (optionBounds.top < menuBounds.top) {
      menu.scrollTop -= menuBounds.top - optionBounds.top;
    }
  }, [activeIndex, open, options]);

  const commit = (raw: string | null) => {
    if (raw === null) return;
    const next = addTag(value, raw);
    if (next === null) {
      if (atLimit) setFeedback(`Достигнут лимит ${MAX_TAGS} тегов. Удалите тег, чтобы добавить другой.`);
      else if (selected.has(normalizeTag(raw))) setDraft("");
      return;
    }
    onChange(next);
    setDraft("");
    setFeedback(null);
    setActiveIndex(-1);
    setOpen(next.length < MAX_TAGS);
    if (next.length < MAX_TAGS) inputRef.current?.focus();
  };

  const remove = (tag: string) => {
    restoreInputFocusRef.current = true;
    onChange(removeTag(value, tag));
    setFeedback(null);
  };

  const keyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") {
      if (open) {
        event.preventDefault();
        event.stopPropagation();
        setOpen(false);
        setActiveIndex(-1);
      }
      return;
    }

    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      if (options.length === 0) return;
      event.preventDefault();
      const delta = event.key === "ArrowDown" ? 1 : -1;
      setOpen(true);
      setActiveIndex((current) => {
        if (current < 0) return delta > 0 ? 0 : options.length - 1;
        return (current + delta + options.length) % options.length;
      });
      return;
    }

    if (event.key === "Enter" || event.key === ",") {
      event.preventDefault();
      commit(resolveTagChoice(draft, activeOption?.value ?? null, suggestions));
      return;
    }

    if (event.key === "Backspace" && draft === "" && value.length > 0) {
      remove(value[value.length - 1]);
    }
  };

  const status = feedback ??
    (atLimit
      ? `Достигнут лимит ${MAX_TAGS} тегов. Удалите тег, чтобы добавить другой.`
      : `${value.length} из ${MAX_TAGS}. Enter добавляет тег; стрелки выбирают подсказку, Backspace в пустом поле убирает последний.`);

  return (
    <div className="tag-input">
      <div
        className="tag-input__chips"
        onClick={(event) => {
          if (!(event.target as HTMLElement).closest("button")) inputRef.current?.focus();
        }}
      >
        {value.map((tag) => (
          <span key={tag} className={`tag tag--target tag--${tagTone(tag)}`}>
            {tag}
            <button
              type="button"
              className="tag-input__remove"
              aria-label={`Убрать тег ${tag}`}
              onClick={() => remove(tag)}
            >
              ×
            </button>
          </span>
        ))}
        <input
          ref={inputRef}
          id={inputId}
          type="text"
          role="combobox"
          aria-label={ariaLabel}
          aria-autocomplete="list"
          aria-expanded={open}
          aria-controls={listId}
          aria-activedescendant={open && activeOption ? `${listId}-${activeIndex}` : undefined}
          aria-describedby={hintId}
          value={draft}
          disabled={atLimit}
          onFocus={() => {
            setOpen(true);
            setFeedback(null);
          }}
          onBlur={() => {
            setOpen(false);
            setActiveIndex(-1);
          }}
          onChange={(event) => {
            setDraft(event.target.value);
            setActiveIndex(-1);
            setOpen(true);
            setFeedback(null);
          }}
          onKeyDown={keyDown}
          placeholder={value.length === 0 ? placeholder : ""}
          autoComplete="off"
        />
      </div>
      <div
        ref={menuRef}
        id={listId}
        className="tag-input__menu"
        role="listbox"
        aria-label="Предложения тегов"
        hidden={!open}
      >
          {options.map((option, index) => (
            <button
              key={`${option.create ? "new" : "suggestion"}-${option.value}`}
              id={`${listId}-${index}`}
              type="button"
              role="option"
              aria-selected={activeIndex === index}
              className={`tag-input__option${activeIndex === index ? " is-active" : ""}`}
              tabIndex={-1}
              onPointerDown={(event: PointerEvent<HTMLButtonElement>) => event.preventDefault()}
              onClick={() => commit(option.value)}
            >
              <span className="tag-input__option-mark" aria-hidden="true">{option.create ? "+" : ""}</span>
              <span>{option.create ? `Создать тег «${option.value}»` : option.value}</span>
              <small>{option.create ? "новый тег" : "есть в проекте"}</small>
            </button>
          ))}
          {options.length === 0 ? (
            <p className="tag-input__empty">
              {query !== "" && selected.has(query)
                ? `Тег «${query}» уже выбран.`
                : "В проекте пока нет других тегов."}
            </p>
          ) : null}
        </div>
      <p id={hintId} className="field-hint" aria-live="polite">{status}</p>
    </div>
  );
}

const TONES = ["teal", "rust", "ochre", "graphite", "moss", "violet"] as const;

export function tagTone(tag: string): (typeof TONES)[number] {
  let hash = 0;
  for (const char of tag) {
    hash = (hash * 31 + char.codePointAt(0)!) % 9973;
  }
  return TONES[hash % TONES.length];
}
