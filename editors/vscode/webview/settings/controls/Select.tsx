import { ChevronIcon } from "../icons";

export function Select({
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
    <div className="set-select">
      <select aria-label={label} value={value} onChange={(e) => onChange(e.target.value)}>
        {/* A value the endpoint reports but the option list does not hold would
            otherwise select the first entry and silently rewrite it. */}
        {options.includes(value) ? null : <option value={value}>{value}</option>}
        {options.map((option) => (
          <option key={option} value={option}>
            {option}
          </option>
        ))}
      </select>
      <ChevronIcon />
    </div>
  );
}
