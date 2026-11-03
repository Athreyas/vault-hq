// "Add a widget": every kind with what it shows; single-use kinds already on Home are off.
import type { WidgetCatalogEntry } from "../../bindings/WidgetCatalogEntry.ts";
import type { WidgetLayout } from "../../bindings/WidgetLayout.ts";
import { canAdd } from "./layout.ts";
import { Modal } from "./WidgetSettings.tsx";

export function CatalogPicker({
  catalog,
  layout,
  onPick,
  onClose,
}: {
  catalog: WidgetCatalogEntry[];
  layout: WidgetLayout;
  onPick: (entry: WidgetCatalogEntry) => void;
  onClose: () => void;
}) {
  return (
    <Modal title="Add a widget" onClose={onClose}>
      <ul className="w-catalog">
        {catalog.map((e) => {
          const ok = canAdd(e, layout);
          return (
            <li key={e.kind}>
              <button type="button" className="w-catalog-item" disabled={!ok} onClick={() => onPick(e)}>
                <b>{e.title}</b>
                <span className="w-muted">{ok ? e.description : "Already on Home."}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </Modal>
  );
}
