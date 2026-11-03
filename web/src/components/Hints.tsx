import type { View } from "../App.tsx";

// The key legend for the current view, always on the last line like a TUI status bar.
const BY_VIEW: Record<View, [string, string][]> = {
  home: [["← → ↑ ↓", "move between panels"], ["⏎", "open panel"]],
  needs: [["↑ ↓", "move"], ["Tab", "choose outcome"]],
  days: [["[ ]", "earlier / later"], ["t", "today"], ["s", "day, 3 days, week"]],
  tasks: [["Tab", "move between fields"], ["⏎", "add task"]],
  inbox: [["Tab", "move between items"]],
  notes: [["f", "filter"], ["↑ ↓ ⏎", "pick"], ["e", "edit"], ["⌘S", "save"], ["r", "folder / recent"], ["o", "show in Finder"]],
  ask: [["⏎", "ask"], ["⇧⏎", "search notes only"]],
};

export default function Hints({ view }: { view: View }) {
  return (
    <footer className="hints">
      <ul>
        <li title="Jump anywhere, ask, search, create or capture">
          <kbd>⌘K</kbd> palette
        </li>
        <li>
          <kbd>/</kbd> ask
        </li>
        <li>
          <kbd>n</kbd> new
        </li>
        <li>
          <kbd>⌘,</kbd> settings
        </li>
        <li>
          <kbd>1-7</kbd> views
        </li>
        {BY_VIEW[view].map(([k, label]) => (
          <li key={k}>
            <kbd>{k}</kbd> {label}
          </li>
        ))}
        <li>
          <kbd>Esc</kbd> close
        </li>
      </ul>
    </footer>
  );
}
