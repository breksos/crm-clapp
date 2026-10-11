// "You have unsaved text on this record." — asked before it can be carried anywhere (W1, QA round 8).

export function DraftGuard({ owner, onKeep, onDiscard }: { owner: string; onKeep: () => void; onDiscard: () => void }) {
  return (
    <div className="draft-guard" role="alertdialog" aria-label="Unsaved text">
      <span>
        You have unsaved text on <code>{owner}</code>. Leaving discards it — it can only be saved there.
      </span>
      <button type="button" className="chip" onClick={onKeep} autoFocus>
        Keep editing
      </button>
      <button type="button" className="chip" onClick={onDiscard}>
        Discard it and go
      </button>
    </div>
  );
}
