import { CloseIcon, SearchIcon } from "./icons";

export function SettingsSearch({
  query,
  onChange,
}: {
  query: string;
  onChange: (next: string) => void;
}) {
  return (
    <div className="set-search">
      <SearchIcon />
      <input
        type="search"
        aria-label="Search settings"
        placeholder="Search settings"
        value={query}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") onChange("");
        }}
      />
      {query && (
        <button
          type="button"
          className="set-search-clear"
          aria-label="Clear search"
          onClick={() => onChange("")}
        >
          <CloseIcon />
        </button>
      )}
    </div>
  );
}
