import { useMemo, useState, type KeyboardEvent } from "react";

interface TagInputProps {
  value: string[];
  onChange: (tags: string[]) => void;
  suggestions?: string[];
  placeholder?: string;
  id?: string;
}

export function normalizeTag(value: string): string {
  return value.trim().toLowerCase();
}

export function TagInput({
  value,
  onChange,
  suggestions = [],
  placeholder = "тег и Enter",
  id,
}: TagInputProps) {
  const [draft, setDraft] = useState("");
  const options = useMemo(() => {
    const query = normalizeTag(draft);
    return suggestions
      .filter((tag) => !value.includes(tag))
      .filter((tag) => query === "" || tag.includes(query))
      .slice(0, 6);
  }, [draft, suggestions, value]);

  const add = (raw: string) => {
    const tag = normalizeTag(raw);
    if (!tag || value.includes(tag) || value.length >= 10) return;
    onChange([...value, tag]);
    setDraft("");
  };

  const remove = (tag: string) => onChange(value.filter((item) => item !== tag));

  const keyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter" || event.key === ",") {
      event.preventDefault();
      add(draft);
    } else if (event.key === "Backspace" && draft === "" && value.length > 0) {
      remove(value[value.length - 1]);
    }
  };

  return (
    <div className="tag-input">
      <div className="tag-input__chips">
        {value.map((tag) => (
          <span key={tag} className={`tag tag--target tag--${tagTone(tag)}`}>
            {tag}
            <button
              type="button"
              aria-label={`Убрать тег ${tag}`}
              onClick={() => remove(tag)}
            >
              ×
            </button>
          </span>
        ))}
        <input
          id={id}
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={keyDown}
          onBlur={() => add(draft)}
          placeholder={value.length === 0 ? placeholder : ""}
          list={options.length > 0 ? `${id ?? "tag"}-options` : undefined}
        />
      </div>
      {options.length > 0 ? (
        <datalist id={`${id ?? "tag"}-options`}>
          {options.map((tag) => (
            <option key={tag} value={tag} />
          ))}
        </datalist>
      ) : null}
      <p className="field-hint">До 10 тегов, Enter или запятая. Все теги в нижнем регистре.</p>
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
