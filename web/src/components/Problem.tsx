import { notBuilt, type HqError } from "../api.ts";

/** A load that failed: "not in this build yet" reads differently from a real error. */
export default function Problem({ error, what }: { error: HqError; what: string }) {
  if (notBuilt(error)) {
    return (
      <div className="empty is-large">
        <p>{what[0]?.toUpperCase() + what.slice(1)} is not in this build yet.</p>
        <p className="empty-hint">{error.message}</p>
      </div>
    );
  }
  return (
    <p className="empty is-error">
      Could not load {what}: {error.message}
    </p>
  );
}
