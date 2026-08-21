import { SettingsSearch } from "./SettingsSearch";
import { SECTIONS, type Section } from "./sections";

/** `active` is null while a search is running: the results cross sections, so
 *  none of them is the one being looked at. */
export function SettingsNav({
  active,
  counts,
  query,
  onQuery,
  onSelect,
}: {
  active: string | null;
  counts: Record<string, number>;
  query: string;
  onQuery: (next: string) => void;
  onSelect: (section: Section) => void;
}) {
  return (
    <nav className="set-nav" aria-label="Settings sections">
      <span className="set-nav-brand">Aster</span>
      <SettingsSearch query={query} onChange={onQuery} />
      <ul>
        {SECTIONS.map((section) => (
          <li key={section.id}>
            <button
              type="button"
              className={section.id === active ? "set-nav-item on" : "set-nav-item"}
              aria-current={section.id === active ? "page" : undefined}
              onClick={() => onSelect(section)}
            >
              <span>{section.label}</span>
              {counts[section.id] > 0 && (
                <span className="set-nav-count" title="Set in this scope">
                  {counts[section.id]}
                </span>
              )}
            </button>
          </li>
        ))}
      </ul>
    </nav>
  );
}
