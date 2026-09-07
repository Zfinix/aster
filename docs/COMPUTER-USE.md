# Computer Use for Aster (2026)

Research notes and strategy for giving aster a desktop computer-use arm.
Compiled 2026-09-06 from arXiv (IDs cited inline) and practitioner sources.
Every number below comes from a cited abstract or a named source. Where a claim
is inference, it says so.

## What "computer use" means in 2026

A computer-use agent (CUA) is a vision-language loop that perceives a real OS
through screenshots and/or UI structure and acts through mouse, keyboard, and
terminal. The 2026 finding that changes the design: **the harness decides, the
model proposes.** Across benchmarks the same model swings by 30+ points when
only the observation and action space changes (ComponentBench, 2608.18307:
GPT-5 mini scores 83.1% with accessibility-tree observations and 48.9% with
coordinate-only pixel control). On MacAgentBench the best configuration beats
the field because of its *skill library*, not its framework or model
(2606.22557). The recurring synthesis in the Efficient GUI Agents survey
(2609.02309) is: selective reading instead of full-context ingestion,
global-to-local visual allocation, recoverable memory instead of raw history
replay, verification-aware control, and hybrid runtimes that switch between GUI
and non-GUI execution.

In 2026 the hard problems are not perception or planning. They are:

1. **Verification.** CUA self-report is not evidence. A capable pipeline scored
   82.9 on OSWorld (above the 72.4 human reference) yet 90% of its 71 failures
   ended in a success claim (CURA, 2608.27808). Models cannot reliably tell
   whether their action landed (Desktop-Delta Bench, 2607.26041) and their
   failure mode is dominated by reasoning and control errors, not perception
   (CUADebug, 2608.02643).
2. **Injection safety.** Indirect prompt injection is now adaptive, multi-step,
   and low-harm. SIR raises attack success from 4% to 24% on a frontier CUA and
   the discovered attacks transfer across models (2608.30207). StepJack shows
   multi-step decomposition adds up to 31 points (2608.06477). II-Bench shows
   "invisible ink" low-harm injections (star a repo, install a package) sail
   past both the model and a simulated human reviewer (2608.02018).
3. **Context cost.** Full screenshots and raw accessibility trees are
   uneconomical. A11y-Compressor cuts input tokens to 22% of raw tree while
   improving OSWorld success by 5.1 points (2605.00551).
4. **Trust of structure over pixels.** Models defer to stale structure when
   pixels and tree disagree, on up to 88% of stale-snapshot probes, and one
   mis-sourced belief at step one compounds with a self-recovery rate of at
   most 3% (Do GUI Agents Believe Their Eyes?, 2607.04334).

## The modern stack everyone is converging on

**Observation:**
- Accessibility/UI tree first: AX (macOS), UIA (Windows), AT-SPI (Linux), DOM
  (web). Compressed, pruned to the task target (A11y-Compressor 2605.00551,
  Weasel target-centered pruning 2605.20291).
- Screenshot as the cross-check and the fallback, not the primary channel.
- Local OCR to label regions the tree misses. Only ~33% of macOS apps expose a
  complete accessibility tree, so the tree alone is not enough (Screen2AX,
  2507.16704).
- A consistency gate: when the tree and the pixels conflict, distrust the
  stale side before acting (2607.04334).

**Action:**
- Semantic element actions (press, focus, set value, open menu) via the
  accessibility API, with element frame coordinates as the fallback, dispatched
  as synthetic input events rather than by moving the real cursor where the OS
  allows it.
- Structured receipts from the dispatcher. A receipt means "the event was
  posted", not "the action happened". Treat every action as unverified until
  re-observed. Never auto-replay on ambiguity.

**Verification:**
- Mandatory post-action observation, decoupled into "what changed" and "did it
  work" (Evidence-First Reflection, 2608.24015: +7.11 reflector accuracy by
  separating change extraction from outcome verification; VisCritic compares
  pre/post screenshots in visual feature space as a process reward,
  2606.24525).
- External runtime monitors over harness telemetry rather than model
  self-report (CURA, 2608.27808): alarm when the trajectory drifts, gate
  mid-execution human oversight on the alarm. CURA's alarm-gated cascade
  recovered 23 of 70 failures while spending only 38 overseer calls.

**Reliability:**
- Reusable skill libraries of recorded, verified routines. Teaching once,
  replaying deterministically, is more reliable than re-planning every run
  (OmegaUse-SOP 2609.02149, Syll 2606.07594, and the MacAgentBench result that
  skill libraries drive most of the win).
- Memory that is externalized and editable: working/episodic/preference
  separation with memory writes as first-class actions (RecVerse 2608.20707),
  local editable artifacts for skills, memory, and governance (Syll).

**Model-side (for those who train, informing what open weights we can host):**
- Environment RL with deterministic reward verification (UI-Venus-2 uses
  visual keypoints plus multi-model voting for RL signals, 2609.00028;
  Mobile-Agent-v3/GUI-Owl 2508.15144; GRPO with expert trajectories nearly
  doubles a 9B agent on one web benchmark, 2607.10079).
- We do not need to train. Aster is provider-agnostic, so when open weights
  reach the frontier (Qwen-CUA class, 2608.02352), they plug in through an
  OpenAI-compatible endpoint. Architecture should never depend on a vendor CUA
  API.

## Tradeoffs that decide the design

| Axis | Option A | Option B | What the evidence says |
| --- | --- | --- | --- |
| Observation | Accessibility tree | Raw screenshot | Tree wins same-model by 34 points (2608.18307) but lies when stale (2607.04334). Use tree + cheap pixel cross-check + OCR for the 67% of apps with no tree. |
| Grounding target | Semantic element | Raw coordinates | Element grounding is stable and self-describing; coordinate grounding of spatial relations is at chance for containment and occlusion (GUI-Primitives, 2608.21832). |
| Screenshots | Full frame every step | Diff crops around action | Diffs plus Set-of-Marks beat full-frame reasoning and cut context (2608.24015). Full screenshots of dense desktop UIs blow the context budget. |
| Action dispatch | Accessibility action | Synthetic mouse/keyboard | AX actions are intent-carrying and survive layout change; synthetic events are the only route when AX is missing. Prefer AX, fall back to synthetic at AX frames, never at model-regressed pixels. |
| Verification | Self-report | External state check | Self-report fails 90% of the time on failure (2608.27808). Deterministic state checks beat LLM judges (2608.30207 oracle, 2607.28609 OSReward, 2607.23263 SeekJudge). |
| Oversight | Confirm every step | Confirm only consequential | Per-step confirmation is the norm and it still loses to invisible-ink injections (2608.02018). Confirmation must restate the concrete side effect and sit behind allowlists. |
| Where it runs | On the user's machine | In a VM/sandbox | On-machine is what users want for personal apps and logins; VM is for eval and risky automation. Support both: the real desktop for consented sessions, a disposable VM for unattended runs. |
| Scope | GUI everything | GUI last | When a CLI or API exists it beats GUI (2606.24551: skills push CLI to 69.3% vs 59.1% GUI). GUI channel exists for the long tail with no other surface. |

## Forgotten and underused technologies that are the moat

The crowded field races on model quality and benchmark scores. The durable
advantages are engineering: channels, permissions, and context economics that
the pixel-clicking demos ignore. Most of these are old, boring, and shipped.

1. **Accessibility notifications as a push sensor.** macOS AX can notify on
   focus change, value change, window move, UI element destruction
   (AXUIElementAddNotificationListener). Every agent I know polls screenshots;
   an agent that *waits* for the value-changed notification knows the action
   landed without burning a frame, and knows it faster. Event-driven
   observation is the single cheapest verification channel on the OS and it is
   nearly unused in CUA research.
2. **Assistive-tech engineering, 30 years old and still the best UI
   abstraction.** Screen readers already solved linearization, element
   navigation, and action dispatch over the same tree we want. The blind-user
   CUA study (2609.00524) shows the demand: screen-reader users issued 1,258
   real commands and even a frontier model only completed 52.5%. An
   a11y-first CUA is not a workaround, it is a feature: it makes aster a
   screen-reader-grade automation tool for everyone.
3. **Hit-testing and menu scripting that never needs pixels.**
   AXUIElementCopyElementAtPosition answers "what is under this point" from
   the OS. Menu bars are traversable AX trees with keyboard equivalents, so
   most commands in most apps are reachable by menu action plus accelerator,
   no screenshot required. This is an example of an a11y-first, pixel-fallback rule.
4. **Synthetic input events that do not move the cursor.** CGEvent-posted
   events (macOS) and the XTEST extension (X11) can dispatch clicks and keys
   without relocating the user's pointer, and CGEvent input needs no screen
   recording permission, only Accessibility. Using an event strategy that does
   not move the real cursor reduces permissions needed, shrinks the attack
   surface, and gives a cleaner consent story.
5. **AppleScript/Apple Events and scripting dictionaries.** Finder, Mail,
   Calendar, Photos, Numbers, and many pro apps expose real object models
   ("tell application X to ...") that are far more reliable than clicking. This
   is RPA-before-RPA and still ships in every macOS. Aster should route GUI
   tasks through the app's scriptable surface when one exists, exactly like it
   routes through a CLI when one exists.
6. **Deterministic replay of recorded, verified steps.** The RPA industry
   (UiPath generation) proved that OCR + coordinates + recorded macros is
   reliable for stable UIs; the failure was brittleness, not concept. Modern
   CUA keeps the recorder and swaps the planner. OmegaUse-SOP records expert
   demonstrations and compiles them to semantic step skills (2609.02149);
   Syll does teachable replay (2606.07594). Record once, verify, then replay
   the verified macro instead of re-planning: this is how you get the
   MacAgentBench skill-library win. Aster's skills system is the natural
   compiler target.
7. **Clipboard as an I/O channel.** Pasting text (with bracketed paste) is
   more reliable than synthesizing keystrokes into hostile input fields, and
   copying a selection is a reliable read of app state when the AX value is
   missing. File drag-and-drop can be emulated by pasting file URLs. Underused
   in every CUA I have seen.
8. **Classic vision: template matching, perceptual hashing, image diff.**
   Before deep grounding models there was OpenCV and pixelmatch. For change
   detection ("did anything move after my click") a perceptual hash over the
   action region is fast, local, and deterministic, and it powers the
   evidence-first reflection step (2608.24015) without a model. Keep a
   screenshot history per app and diff in time, not in model context.
9. **Local OCR (macOS Vision, Tesseract).** Labeling a screenshot locally
   costs nothing, sends no pixels to a provider, and fills the gap Screen2AX
   found: 67% of macOS apps have an incomplete or missing tree. OCR text plus
   AX roles plus frames gives a synthesized tree where the real one is absent.
10. **Per-app UI memory as files.** Stable AX identifiers, learned layout
    regions, and routine definitions are per-app artifacts. Store them outside
    context as editable local files (Weasel prunes to targets, 2605.20291;
    Syll externalizes everything, 2606.07594) and inject on demand. This is
    the same move aster already made with skills and memory: registry of small
    files, bounded render, retrieval on demand.
11. **The terminal as the universal accessibility layer.** GUI agents that can
    switch to CLI/API execution when it exists dominate GUI-only agents
    (2606.24551, MacAgentBench's GUI+CLI tasks, the hybrid-runtime finding in
    2609.02309). Aster already is that half. The CUA arm only needs to be the
    second half, reached when there is no other surface.
12. **CDP and WebDriver for embedded webviews.** Browsers are solved because
    DOM is a free accessibility tree. Many desktop apps are Electron or WebKit
    shells; when the app is a webview, drive the DOM through CDP before
    touching AX or pixels. (The repo already scaffolds browser-use, so the
    browser channel exists; the desktop arm completes it.)
13. **Headless virtual displays.** Xvfb/Xvnc, and macOS virtual displays, give
    a deterministic screen for unattended automation and eval, and let
    synthetic input land without a real monitor. Every serious desktop-agent
    eval uses VMs or headless displays; so should aster's harness, with the
    user's real desktop reserved for consented interactive sessions.
14. **Termination discipline.** The newest failure class is over-compliance:
    agents keep executing when the instruction is infeasible or conflicts with
    the UI (CONFLICTGUI, 2609.03438). "Know when not to act" is a trainable,
    gateable behavior (their CONFLICTGUARD adds a feasibility verification
    step before action generation). An aster CUA should treat "stop and ask"
    as a first-class action with a low threshold.

## Where aster is today

Aster already has most of the hard substrate, because the CUA problem is
largely the agent problem:

- **Policy-gated execution with a real permission model.** Tools are gated,
  modes exist (aster_policy), and user-facing copy is plain language. A
  desktop arm slots into the same gate rather than inventing a new one.
- **A context system built for economy.** Bounded injections with hard caps,
  append-only history, cache-friendly ordering, summarize-and-replace only via
  one audited path, and a progressive-injection pattern in aster-mcp
  (inventory under a token budget, search to expand, estimated_tokens). Raw
  accessibility trees and full screenshots are exactly the kind of unbounded
  input this system was built to refuse. Compression and pruning (A11y-
  Compressor, Weasel) are the interface, not the feature.
- **Image parts already plumbed.** aster-ai carries images with an invariant:
  never strip images when captioning fails, because a dead vision model must
  not blank an image the provider could have taken. CUA screenshots inherit
  that invariant for free.
- **MCP as the capability boundary.** In-process MCP crates per capability,
  tool-level filtering, progressive injection. A desktop server (observe,
  input, verify) is another crate following the same shape.
- **Skills, sessions, transcripts, cron, remote.** The skill system is the
  compile target for recorded routines; the transcript and session format is
  the audit log; cron and the remote channel turn "computer use" into
  unattended, always-on operation.
- **A review culture and a mock-provider harness.** Injection red-teaming in
  CI (SIR-style adaptive attacks against the mock provider) is a natural
  extension of the existing e2e harness. Desktop CUA is the one place where
  the whole field agrees safety is the bottleneck, and aster is one of the few
  agents whose DNA is honesty and verification.

What aster does not yet have: screen capture, UI tree ingestion, synthetic
input, accessibility-notification listening, per-app UI memory, the macOS
permission flow for Accessibility and Screen Recording, and a desktop eval
harness. Those are the build.

## Recommended architecture: terminal first, a11y second, pixels third

**Routing (hybrid runtime).** Before any GUI move, try in order: CLI, API/MCP,
AppleScript/scripting dictionary, CDP for embedded webviews, then the desktop
CUA channel. The CUA channel is a capability of last resort, not the default.
This is the single biggest reliability and cost lever available, and aster's
terminal nature means it is already good at the first three.

**Observation protocol.**
- Primary: a pruned, compressed accessibility tree, injected progressively
  (per-app inventory under a budget, expand on demand). This is aster-mcp's
  existing pattern applied to a UI tree.
- Cross-check: one cheap diff against the previous frame to prove the screen
  matches the tree. When pixels and tree conflict, distrust the tree
  (2607.04334) and say so in the transcript.
- Fallback: local OCR over the diff region to synthesize labels for the apps
  with no tree (Screen2AX's 67%).
- Events: subscribe to AX notifications (focus, value, window) for the active
  app. Wait for the notification after an action before deciding what to
  observe next; only screenshot when no notification arrives.

**Action protocol.**
- Element actions first: press, focus, set value, open menu, via AX.
- Fallback: synthetic events at the element's frame (CGEvent/XTEST), never at
  model-regressed pixels. Prefer event dispatch that does not move the real
  cursor.
- Text entry via clipboard paste when key synthesis is flaky.
- Every dispatch returns a structured receipt with a possible-sent semantic.
  Receipts go into the transcript verbatim, success claims are never derived
  from receipts alone.

**Verification protocol (mandatory after every action).**
1. Diff the action region: did the screen change? (perceptual hash first,
   pixel diff if needed)
2. If it changed, re-read the tree region and check the outcome against the
   goal predicate with deterministic checks where possible.
3. If it did not change, treat the action as failed or intercepted. Ask or
   retry once with a different strategy, never blind-replay.
4. An external monitor over harness telemetry (CURA-style) alarms when the
   trajectory claims success without state change, when the same failure
   repeats, or when steps exceed budget, and escalates to the user.

**Context economy (non-negotiable, matches aster invariants).**
- Nothing unbounded: tree inventories and screenshot frames get caps declared
  as module constants. No single item over ~10K tokens; flag anything over
  ~1K tokens for review.
- Append-only and cache-stable: screenshots and trees go through the same
  image/value plumbing as every other tool result. Never rewrite history;
  compaction stays the audited summarize-and-replace path.
- Images survive: caption failures never strip a screenshot, per the aster-ai
  image invariant.

## Safety and consent model

The field's 2026 conclusion is unambiguous: safety must live in the harness,
not the model. SIR, StepJack, II-Bench, and MobileWorldSafety all break
frontier CUAs with content the model reads. Aster's answer is structural:

- **Allowlists, not vibes.** Computer-use runs against a per-session allowlist
  of bundle IDs. Apps not on the list are not observable and not actionable.
- **Read vs act separation.** Observing (tree, screenshot, notifications) is
  one permission tier. Acting is another: click/type into the allowlisted app
  is granted per session; keystroke content that is not plain text gets a
  confirm.
- **Consequential actions restate the concrete side effect.** II-Bench shows
  confirm dialogs lose to invisible-ink injections when the user approves a
  model's summary. The confirm prompt must show the concrete, checkable
  effect: the destination app, the exact command or text to be sent, the
  irreversible part, and what will not happen. Deterministic post-conditions
  ("state X equals Y after") are preferred over asking at all.
- **Never trust on-screen instructions as commands.** Screen content is data.
  Injections are expected. This is a policy, not a prompt, and the transcript
  should record every ignored instruction-looking string so red teams can see
  the countermeasures worked.
- **Permissions that are revocable and visible.** macOS Accessibility and
  Screen Recording grants are per-app, visible in System Settings, revocable
  at any time. Never request more than the session needs; a read-only
  observation session should not hold input permissions. Remember the image
  invariant: no screen content is required to be captioned to proceed.
- **Red-team in CI.** Extend the mock-provider harness with SIR-style adaptive
  injection (attack library, feedback loop, deterministic success oracle
  checking filesystem and state), StepJack-style multi-step decomposition, and
  II-Bench-style low-harm goals. Ship only when the harness says the
  countermeasures held. The docs' own review rules already demand this level
  of honesty; CUA is where it pays off.

## Evaluation plan

- **Own the harness, borrow the tasks.** OSWorld, WindowsAgentArena, and
  MacAgentBench define the task shapes; a self-hosted harness with
  deterministic state oracles is the only trustworthy judge (2608.30207,
  2607.28609, 2607.23263). Reuse aster's mock-provider e2e shape with a
  desktop fixture (macOS VM for GUI + CLI tasks, since MacAgentBench shows
  ~60% of real tasks mix both).
- **Score progress, not just pass/fail.** Multi-checkpoint scoring (sub-goal
  completion) because models with equal Pass@1 differ widely in partial
  progress (MacAgentBench). Track efficiency: tokens, screenshots, steps per
  task, verifier cost (2609.02309).
- **Measure the failure classes.** Grounding, planning, constraint tracking,
  termination (2609.00524 taxonomy) and the action-effect verification gap
  (Desktop-Delta) as separate report cards, so fixes target the real deficit.
- **Keep a benchmark-scoring honesty rule.** Benchmarks mis-score failures
  (2607.28367 found 15.3% of FAIL verdicts wrong); never report a number from
  a judge you do not control.

## Anti-patterns (what not to build)

- A screenshot-first, full-frame CUA loop. It spends the context budget and
  scores worse than tree-first with the same model.
- Vendor-lock: depending on a proprietary computer-use API contract instead of
  provider-agnostic observation and action primitives.
- Trusting the model's success claim, or the platform's event receipt, as
  ground truth.
- Auto-replaying an action because the outcome is ambiguous.
- Confirm dialogs that ask the user to approve a model-generated summary
  without restating concrete side effects.
- Growing aster-cli with the CUA loop. The desktop server is a library/MCP
  concern (a new crate or an extension of aster-mcp), with aster-cli doing
  wiring and terminal I/O only, per the repo rule that pushes logic out of
  aster-cli.
- Unbounded UI trees or raw screenshot history in context, which violates the
  model-visible context invariants.

## Build order

- **v0: observe and act on the real desktop.** A desktop MCP crate with:
  pruned AX tree inventory + progressive injection, diff screenshot capture,
  local OCR, AX element actions, synthetic input fallback, receipts, all
  behind aster policy with an app allowlist. This alone makes aster able to
  click, type, and read inside allowed apps on macOS.
- **v1: the verification loop and per-app memory.** AX notification listener,
  action-region diffing, evidence-first outcome checks, external drift
  monitor, per-app UI memory files. Reliability beats features here.
- **v2: recorded routines as skills.** Human demonstrates once, aster records
  the verified trace, compiles it to a guarded skill with checkpoints and
  deterministic post-conditions, replays it through the normal skills system.
  This is the MacAgentBench win: skill library over raw planning.
- **v3: unattended and evaluated.** Disposable VM/headless display harness,
  OSWorld-style and MacAgentBench-style checkpoints with deterministic
  oracles, injection red-team in CI, cron + remote for always-on operation.

## Open questions

- Linux and Windows trees (AT-SPI, UIA) differ from macOS AX. v0 should
  define the observation format abstractly so the OS backends can grow later,
  but macOS first is the honest scope given this machine.
- Where the desktop server lives: a new crate (aster-desktop) vs extending
  aster-mcp. My read of the repo rules: a new crate with an MCP surface, with
  aster-mcp unchanged, keeps the core dependency-light and aster-cli small.
- Permission ergonomics: how a consent flow reads in the TUI and in the
  desktop app, in plain user language, without nagging. The repo's
  user-facing-copy rule applies; concrete copy needs design.

## Source index (arXiv IDs cited above)

- Surveys and landscape: 2501.16150, 2411.18279, 2504.20464, 2510.16720,
  2609.02309
- Systems and generalist CUAs: 2403.03186, 2410.08164, 2504.00906,
  2410.18603, 2501.10893, 2501.12326, 2509.02544, 2608.02352, 2609.00028,
  2508.15144, 2604.08516
- Observation and grounding: 2608.18307, 2605.00551, 2605.20291, 2607.04334,
  2608.21832, 2608.21794, 2507.16704, 2408.00203, 2411.17465, 2505.00684,
  2510.16051
- Verification and reflection: 2608.27808, 2608.24015, 2606.24525,
  2607.26041, 2608.02643, 2609.03438, 2607.28609, 2607.23263, 2607.28367
- Reliability and memory: 2609.02149, 2606.07594, 2608.20707, 2606.22557,
  2607.10079, 2609.00524
- Safety and injection: 2608.30207, 2608.06477, 2608.02018, 2608.17659,
  2606.22864, 2511.19477
- Training signal (informative only): 2609.02987, 2609.02401, 2606.24551
