import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { Effort, Provider } from "../lib/types";
import { EFFORT_OPTIONS } from "../lib/effort";
import { modelChip } from "../lib/model";
import { ChoiceList } from "./ChoiceList";
import { ChoiceSlider } from "./ChoiceSlider";
import { CloudIcon, CubeIcon, GaugeIcon } from "./icons";
import { ModelPicker } from "./ModelPicker";

type Pane = "model" | "provider";

/** The model chip's menu: three rows, each opening its list beside them under
 *  the pointer. One list is out at a time, so the panel reads as one surface
 *  changing shape rather than every setting shouting at once. */
export function ModelMenu({
  pane: initial,
  model,
  models,
  recommended,
  recent,
  loading,
  error,
  effort,
  providers,
  onSelect,
  onRefresh,
  onRefreshProviders,
  onEffort,
  onProvider,
  onClose,
}: {
  pane: Pane | null;
  model: string | null;
  models: string[];
  recommended: string[];
  recent: string[];
  loading: boolean;
  error?: string;
  effort: Effort | null;
  providers: Provider[];
  onSelect: (model: string) => void;
  onRefresh: () => void;
  onRefreshProviders: () => void;
  onEffort: (effort: Effort | null) => void;
  onProvider: (provider: Provider) => void;
  onClose: () => void;
}) {
  const inner = useRef<HTMLDivElement>(null);
  const [pane, setPane] = useState<Pane | null>(initial);
  const [height, setHeight] = useState(0);

  useEffect(() => {
    onRefreshProviders();
  }, []);

  useLayoutEffect(() => {
    const box = inner.current;
    if (!box) return;
    const observer = new ResizeObserver(([entry]) => setHeight(entry.contentRect.height));
    observer.observe(box);
    return () => observer.disconnect();
  }, [pane]);

  const current = providers.find((p) => p.current);

  const row = (id: Pane, icon: React.ReactNode, label: string, value: string) => (
    <button
      className="picker-row model-row"
      data-active={pane === id}
      aria-haspopup="menu"
      aria-expanded={pane === id}
      onMouseEnter={() => setPane(id)}
      onFocus={() => setPane(id)}
      onClick={() => setPane(id)}
    >
      {icon}
      <span className="picker-body">
        <span className="picker-label">{label}</span>
      </span>
      <span className="model-row-value">{value}</span>
    </button>
  );

  const leave = (e: React.MouseEvent<HTMLDivElement>) => {
    if (e.currentTarget.contains(document.activeElement)) return;
    setPane(null);
  };

  return (
    <div className="model-menu" onMouseLeave={leave}>
      <div className="picker model-root" role="menu" aria-label="Turn settings">
        {row("model", <CubeIcon />, "Model", modelChip(model))}
        {row("provider", <CloudIcon />, "Provider", current?.name ?? "")}
        <div className="picker-row model-row model-row-choice">
          <GaugeIcon />
          <span className="picker-body">
            <span className="picker-label">Effort</span>
          </span>
          <ChoiceSlider
            label="Effort"
            options={EFFORT_OPTIONS}
            value={effort ?? ""}
            onSelect={(value) => onEffort((value || null) as Effort | null)}
          />
        </div>
      </div>

      <div className="model-flyout" data-open={pane !== null} style={{ height: pane ? height : 0 }}>
        <div className="model-flyout-inner" key={pane ?? "none"} ref={inner}>
          {pane === "model" && (
            <ModelPicker
              model={model}
              models={models}
              recommended={recommended}
              recent={recent}
              loading={loading}
              error={error}
              onSelect={onSelect}
              onRefresh={onRefresh}
              onClose={onClose}
            />
          )}

          {pane === "provider" && (
            <ChoiceList
              label="Provider"
              empty="Loading the catalog…"
              choices={providers.map((provider) => ({
                key: provider.base_url,
                label: provider.name,
                title: provider.base_url,
                checked: provider.current,
                onSelect: () => {
                  onProvider(provider);
                  onClose();
                },
              }))}
            />
          )}
        </div>
      </div>
    </div>
  );
}
