/** A choice small enough to show every option at once. Wider sets get a
 *  `Select` instead; `Choice.tsx` decides which. */
export function Segmented({
  options,
  value,
  label,
  onChange,
}: {
  options: string[];
  value: string;
  label: string;
  onChange: (next: string) => void;
}) {
  return (
    <div className="set-segmented" role="radiogroup" aria-label={label}>
      {options.map((option) => (
        <button
          type="button"
          key={option}
          role="radio"
          aria-checked={option === value}
          className={option === value ? "set-segment on" : "set-segment"}
          onClick={() => onChange(option)}
        >
          {option}
        </button>
      ))}
    </div>
  );
}
