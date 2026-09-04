export interface Choice {
  key: string;
  label: string;
  title?: string;
  checked: boolean;
  onSelect: () => void;
}

/** A list of one-line options with a tick on the one in force. The panels that
 *  need icons or a second line render their own rows. */
export function ChoiceList({
  label,
  choices,
  empty,
}: {
  label: string;
  choices: Choice[];
  empty?: string;
}) {
  return (
    <div className="picker" role="menu" aria-label={label}>
      {choices.length === 0 && empty && <div className="picker-empty">{empty}</div>}
      {choices.map((choice) => (
        <button
          key={choice.key}
          className="picker-row"
          role="menuitemradio"
          aria-checked={choice.checked}
          title={choice.title}
          onClick={choice.onSelect}
        >
          <span className="picker-body">
            <span className="picker-label">{choice.label}</span>
          </span>
          {choice.checked && <span className="picker-check">✓</span>}
        </button>
      ))}
    </div>
  );
}
