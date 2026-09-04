import type { ConfigKey, ConfigValue, Provider } from "../../src/protocol";
import { asList } from "./sections";
import { ChipList } from "./controls/ChipList";
import { Combo } from "./controls/Combo";
import { NumberInput } from "./controls/NumberInput";
import { Segmented } from "./controls/Segmented";
import { Select } from "./controls/Select";
import { TextInput } from "./controls/TextInput";
import { Toggle } from "./controls/Toggle";

const SEGMENTED_BUDGET = 34;

const MODEL_KEYS = new Set([
  "review.model",
  "review.hypothesis_model",
  "review.verify_model",
  "agents.collector_model",
]);

export function Control({
  keyRow,
  shown,
  models,
  providers,
  onCommit,
}: {
  keyRow: ConfigKey;
  shown: ConfigValue;
  models: string[];
  providers: Provider[];
  onCommit: (next: Exclude<ConfigValue, null>) => void;
}) {
  const label = keyRow.label;
  const text = typeof shown === "string" ? shown : "";

  if (MODEL_KEYS.has(keyRow.key)) {
    return (
      <Combo
        value={text}
        options={models.map((id) => ({ id }))}
        label={label}
        placeholder={keyRow.default}
        onCommit={onCommit}
      />
    );
  }

  if (keyRow.key === "review.base_url") {
    return (
      <Combo
        value={text}
        options={providers.map((p) => ({ id: p.base_url, detail: p.name }))}
        label={label}
        placeholder={keyRow.default}
        onCommit={onCommit}
      />
    );
  }

  switch (keyRow.kind) {
    case "bool":
      return <Toggle checked={shown === true} label={label} onChange={onCommit} />;

    case "choice": {
      const width = keyRow.choices.join("").length;
      return width <= SEGMENTED_BUDGET ? (
        <Segmented options={keyRow.choices} value={text} label={label} onChange={onCommit} />
      ) : (
        <Select options={keyRow.choices} value={text} label={label} onChange={onCommit} />
      );
    }

    case "number":
      return (
        <NumberInput
          value={typeof shown === "number" ? shown : null}
          label={label}
          unit={keyRow.unit}
          onCommit={onCommit}
        />
      );

    case "list":
      return (
        <ChipList
          items={asList(shown)}
          label={label}
          placeholder={placeholderFor(keyRow.key)}
          onChange={onCommit}
        />
      );

    default:
      return (
        <TextInput
          value={text}
          label={label}
          placeholder={keyRow.default}
          mono
          onCommit={onCommit}
        />
      );
  }
}

function placeholderFor(key: string): string {
  switch (key) {
    case "permissions.allow":
    case "permissions.ask":
    case "permissions.deny":
      return "Bash(cargo test:*)";
    case "permissions.additional_directories":
      return "../shared";
    case "review.include":
    case "review.exclude":
      return "src/**/*.rs";
    case "review.analyzers":
      return "semgrep";
    case "review.focus_areas":
      return "concurrency";
    default:
      return "Add an entry";
  }
}
