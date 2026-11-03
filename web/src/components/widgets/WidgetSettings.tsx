// A widget's settings form, drawn from the schema its catalog entry carries. The server
// validates again on save (and says why if a value is refused).
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import type { ConfigField } from "../../bindings/ConfigField.ts";
import type { WidgetCatalogEntry } from "../../bindings/WidgetCatalogEntry.ts";
import type { WidgetInstance } from "../../bindings/WidgetInstance.ts";
import { errorText, widgetData } from "../../api/widgets.ts";

type Config = Record<string, unknown>;

/** A modal over the grid; the native <dialog> keeps focus inside and closes on Escape. */
export function Modal({ title, onClose, children }: { title: string; onClose: () => void; children: ReactNode }) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
    return () => d?.close();
  }, []);
  return (
    <dialog
      ref={ref}
      className="w-dialog"
      aria-labelledby={titleId}
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      onClick={(e) => e.target === e.currentTarget && onClose()}
    >
      <div className="w-dialog-body">
        <header className="w-dialog-head">
          <h2 id={titleId}>{title}</h2>
          <button type="button" className="link-button" onClick={onClose} aria-label="Close">
            ✕
          </button>
        </header>
        {children}
      </div>
    </dialog>
  );
}

export function WidgetSettings({
  instance,
  entry,
  onSave,
  onClose,
}: {
  instance: WidgetInstance;
  entry: WidgetCatalogEntry;
  onSave: (config: Config) => Promise<string | null>;
  onClose: () => void;
}) {
  const [draft, setDraft] = useState<Config>(() => ({ ...entry.defaultConfig, ...structuredClone(instance.config) }));
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  return (
    <Modal title={`${entry.title} settings`} onClose={onClose}>
      <form
        className="w-form"
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          const err = await onSave(draft);
          setBusy(false);
          if (err) setError(err);
          else onClose();
        }}
      >
        {entry.fields.map((f) => (
          <Field key={f.key} field={f} value={draft[f.key]} onChange={(v) => setDraft((d) => ({ ...d, [f.key]: v }))} />
        ))}
        {error && (
          <p className="w-note is-bad" role="alert">
            {error}
          </p>
        )}
        {preview && (
          <p className="w-note" role="status">
            {preview}
          </p>
        )}
        <footer className="w-form-foot">
          {entry.kind === "json_status" && (
            <button
              type="button"
              className="button"
              disabled={busy}
              onClick={async () => {
                setPreview("…");
                try {
                  setPreview((await widgetData(entry.kind, draft)).summary);
                } catch (e) {
                  setPreview(errorText(e));
                }
              }}
            >
              try it
            </button>
          )}
          <button type="button" className="button" onClick={onClose}>
            cancel
          </button>
          <button className="button is-primary" disabled={busy}>
            save
          </button>
        </footer>
      </form>
    </Modal>
  );
}

function Field({ field, value, onChange }: { field: ConfigField; value: unknown; onChange: (v: unknown) => void }) {
  const id = useId();
  const input = field.input;
  const help = field.help ? <small id={`${id}-help`}>{field.help}</small> : null;
  const describedBy = field.help ? `${id}-help` : undefined;

  if (input.type === "toggle") {
    return (
      <div className="w-field is-toggle">
        <input id={id} type="checkbox" checked={value === true} onChange={(e) => onChange(e.target.checked)} aria-describedby={describedBy} />
        <label htmlFor={id}>{field.label}</label>
        {help}
      </div>
    );
  }

  if (input.type === "list") {
    const rows = Array.isArray(value) ? (value as Config[]) : [];
    const set = (i: number, row: Config) => onChange(rows.map((r, j) => (j === i ? row : r)));
    return (
      <fieldset className="w-field is-list">
        <legend>{field.label}</legend>
        {help}
        {rows.map((row, i) => (
          <div key={i} className="w-list-row">
            {input.fields.map((sub) => (
              <Field key={sub.key} field={sub} value={row[sub.key]} onChange={(v) => set(i, { ...row, [sub.key]: v })} />
            ))}
            <button type="button" className="link-button" onClick={() => onChange(rows.filter((_, j) => j !== i))} aria-label={`Remove ${field.label.toLowerCase()} row ${i + 1}`}>
              ✕
            </button>
          </div>
        ))}
        {rows.length < input.maxItems && (
          <button type="button" className="link-button" onClick={() => onChange([...rows, Object.fromEntries(input.fields.map((s) => [s.key, ""]))])}>
            + add
          </button>
        )}
      </fieldset>
    );
  }

  let control: ReactNode;
  switch (input.type) {
    case "text":
      control = <input id={id} type="text" value={String(value ?? "")} maxLength={input.maxLen} placeholder={input.placeholder} onChange={(e) => onChange(e.target.value)} aria-describedby={describedBy} spellCheck={false} />;
      break;
    case "url":
      control = <input id={id} type="url" value={String(value ?? "")} placeholder={input.placeholder} onChange={(e) => onChange(e.target.value)} aria-describedby={describedBy} spellCheck={false} />;
      break;
    case "number":
      control = (
        <span className="w-number">
          <input
            id={id}
            type="number"
            value={typeof value === "number" ? value : ""}
            min={input.min}
            max={input.max}
            step={input.step}
            onChange={(e) => onChange(e.target.value === "" ? null : Number(e.target.value))}
            aria-describedby={describedBy}
          />
          {input.unit && <span className="w-muted">{input.unit}</span>}
        </span>
      );
      break;
    case "select":
      control = (
        <select id={id} value={String(value ?? "")} onChange={(e) => onChange(e.target.value)} aria-describedby={describedBy}>
          {input.options.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      );
      break;
  }
  return (
    <div className="w-field">
      <label htmlFor={id}>{field.label}</label>
      {control}
      {help}
    </div>
  );
}
