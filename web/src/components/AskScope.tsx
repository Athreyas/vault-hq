import type { SearchScope } from "../bindings/SearchScope.ts";
import type { VaultInfo } from "../bindings/VaultInfo.ts";
import { DAY_SPANS } from "./AskState.ts";

const KINDS = [
  { kinds: "both", label: "both" },
  { kinds: "notes", label: "notes" },
  { kinds: "bookmarks", label: "bookmarks" },
] as const;

/** Which vaults, which days and which kinds of note a question reads. */
export default function AskScope({ scope, vaults, onChange }: { scope: SearchScope; vaults: VaultInfo[]; onChange: (s: SearchScope) => void }) {
  const days = scope.days ?? 0;
  const toggleVault = (id: string) => {
    // an empty list means every vault
    const current = scope.vaults.length ? scope.vaults : vaults.map((v) => v.id);
    const next = current.includes(id) ? current.filter((v) => v !== id) : [...current, id];
    if (next.length === 0) return; // keep at least one vault
    onChange({ ...scope, vaults: next.length === vaults.length ? [] : next });
  };
  return (
    <div className="ask-scope" role="group" aria-label="Scope">
      <span className="label">scope</span>
      <div className="segmented" role="radiogroup" aria-label="Kinds">
        {KINDS.map((k) => (
          <button key={k.kinds} type="button" role="radio" aria-checked={scope.kinds === k.kinds} className={scope.kinds === k.kinds ? "is-active" : ""} onClick={() => onChange({ ...scope, kinds: k.kinds })}>
            {k.label}
          </button>
        ))}
      </div>
      <label className="sr-only" htmlFor="ask-days">
        Days
      </label>
      <select id="ask-days" value={days} onChange={(e) => onChange({ ...scope, days: Number(e.target.value) || undefined })}>
        {DAY_SPANS.map((s) => (
          <option key={s.days} value={s.days}>
            {s.label}
          </option>
        ))}
      </select>
      {days > 0 && (
        <>
          <label htmlFor="ask-until" className="ask-scope-faint">
            ending
          </label>
          <input id="ask-until" type="date" value={scope.until ?? ""} onChange={(e) => onChange({ ...scope, until: e.target.value || undefined })} />
        </>
      )}
      {vaults.length > 1 && (
        <span className="ask-scope-vaults">
          {vaults.map((v) => (
            <label key={v.id}>
              <input type="checkbox" checked={scope.vaults.length === 0 || scope.vaults.includes(v.id)} onChange={() => toggleVault(v.id)} />
              {v.name}
            </label>
          ))}
        </span>
      )}
    </div>
  );
}
