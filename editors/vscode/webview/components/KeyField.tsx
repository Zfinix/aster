/** One line for a pasted secret: masked, monospace, Enter commits. */
export function KeyField({
  value,
  placeholder,
  disabled = false,
  invalid = false,
  onChange,
  onCommit,
}: {
  value: string;
  placeholder: string;
  disabled?: boolean;
  invalid?: boolean;
  onChange: (value: string) => void;
  onCommit: () => void;
}) {
  return (
    <input
      type="password"
      className="setup-key"
      value={value}
      placeholder={placeholder}
      disabled={disabled}
      aria-invalid={invalid || undefined}
      autoFocus
      autoComplete="off"
      autoCorrect="off"
      autoCapitalize="off"
      spellCheck={false}
      onChange={(e) => onChange(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter" && value.trim()) {
          e.preventDefault();
          onCommit();
        }
      }}
    />
  );
}
