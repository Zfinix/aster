/** Vetted for agentic coding, strongest first then down the cost curve; every
    one takes tools, which a harness that calls them is useless without. Ids are
    OpenRouter's; another endpoint falls back to its own catalog. */
export const MODELS = [
  "anthropic/claude-opus-5",
  "anthropic/claude-sonnet-5",
  "openai/gpt-5.6-sol",
  "deepseek/deepseek-v4-pro",
  "z-ai/glm-5.3",
  "moonshotai/kimi-k2.7-code",
  "google/gemini-3.7-flash",
  "qwen/qwen3-coder",
  "minimax/minimax-m2.5",
  "deepseek/deepseek-v4-flash",
  "poolside/laguna-xs-2.1",
];

/** The five shown at the top of the picker; the rest of MODELS stays searchable. */
export const RECOMMENDED = MODELS.slice(0, 5);
