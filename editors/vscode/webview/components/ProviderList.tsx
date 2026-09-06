import { useState } from "react";
import type { Provider } from "../../src/protocol";
import { providerDetail, providerLabel, shortlist } from "../lib/providers";

/** The catalog as rows inside the card, the picker's own rows so it reads like
 *  every other list in the panel. The long tail waits behind one more row. */
export function ProviderList({
  providers,
  disabled = false,
  onPick,
}: {
  providers: Provider[];
  disabled?: boolean;
  onPick: (provider: Provider) => void;
}) {
  const [more, setMore] = useState(false);
  const { first, rest } = shortlist(providers);

  if (providers.length === 0) {
    return <div className="picker-empty">Loading providers…</div>;
  }

  const row = (provider: Provider) => (
    <button
      key={provider.base_url}
      type="button"
      className="picker-row"
      disabled={disabled}
      onClick={() => onPick(provider)}
    >
      <span className="picker-body">
        <span className="picker-label">{providerLabel(provider).label}</span>
        <span className="picker-detail">{providerDetail(provider)}</span>
      </span>
    </button>
  );

  return (
    <div className="setup-providers" role="list" aria-label="Providers">
      {first.map(row)}
      {more && rest.map(row)}
      {!more && rest.length > 0 && (
        <button
          type="button"
          className="picker-row setup-more"
          disabled={disabled}
          onClick={() => setMore(true)}
        >
          <span className="picker-body">
            <span className="picker-label">More providers</span>
            <span className="picker-detail">{rest.length} more</span>
          </span>
        </button>
      )}
    </div>
  );
}
