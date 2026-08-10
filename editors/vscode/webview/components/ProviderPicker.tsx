import { useEffect, useRef } from "react";
import type { Provider } from "../../src/protocol";
import { useDismiss } from "../lib/dismiss";
import { post } from "../lib/host";

/**
 * The `/provider` picker. Choosing one repoints this panel's runs at that
 * endpoint and adopts its example model, the way the TUI's does: a provider
 * kept without a model it serves would fail at the next turn instead.
 */
export function ProviderPicker({
  providers,
  onSelect,
  onClose,
}: {
  providers: Provider[];
  onSelect: (provider: Provider) => void;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useDismiss(ref, onClose);

  useEffect(() => {
    post({ type: "listProviders" });
  }, []);

  return (
    <div className="picker" ref={ref} role="menu" aria-label="Provider">
      <div className="picker-head">Provider</div>
      {providers.length === 0 && <div className="picker-empty">Loading the catalog…</div>}
      {providers.map((provider) => (
        <button
          key={provider.base_url}
          className="picker-row"
          role="menuitemradio"
          aria-checked={provider.current}
          title={provider.base_url}
          onClick={() => {
            onSelect(provider);
            onClose();
          }}
        >
          <span className="picker-body">
            <span className="picker-label">{provider.name}</span>
            <span className="picker-detail">{provider.base_url}</span>
          </span>
          {provider.current && <span className="picker-check">✓</span>}
        </button>
      ))}
    </div>
  );
}
