# Dayflow reference harness

This folder runs the real Dayflow analysis code (the macOS app Daytrace is ported from, Swift, MIT)
on a GitHub-hosted macOS runner, feeds it a fixed set of synthetic screenshots, and saves what it
produces. The same frames go through Daytrace's replay test, and `compare.py` prints the two
side by side.

| Path | What it is |
|---|---|
| `dayflow-harness/ReplayHarnessTests.swift` | XCTest that the workflow copies into `Dayflow/DayflowTests` |
| `dayflow-harness/build_fix.py` | Build fix for Xcode 26.3, applied to the CI checkout (see below) |
| `frames/` | 180 synthetic JPEGs (1280x720) and `manifest.json` |
| `compare.py` | Side-by-side of a harness run and a Daytrace replay |
| `fetch_log_output.py` | Rebuilds the output folder from the job log |
| `../.github/workflows/dayflow-reference.yml` | The macOS workflow |

Dayflow is pinned to commit `e90d25dc633fcaa2f433042e82defe4dfbabc5a4` (v2.7.1 appcast,
2026-10-15), set as `DAYFLOW_SHA` in the workflow.

## What runs

Dayflow has several analysis paths. The harness uses the OpenAI-compatible one
(`OpenAICompatibleProvider`), because it is the path Daytrace's cloud pipeline follows: one
multi-image transcription request per 15-minute batch, then one card request over the 45-minute
lookback window. The provider takes an injectable completion closure (Dayflow's own
`OpenAICompatibleProviderTests` use it), so the test can answer requests itself or forward them.

Dayflow code that runs unchanged:

- frame sampling (at most 15 frames per batch, first and last included), JPEG re-encoding
  (height 720, quality 0.85) and the per-frame `Screenshot at HH:MM:SS:` labels
- the transcription and card prompts, the category list (`CategoryStore.descriptorsForLLM`,
  default categories on a fresh runner) and the system message
- the retry loop (three attempts, `PREVIOUS ATTEMPT FAILED. Fix: ...`), JSON fence stripping and
  parsing, segment validation, category normalization, card coverage and duration validation

Copied into the test, in memory, because the hosted app runs its own analysis job against its
SQLite store at launch and must not pick up these frames:

- batching from `AnalysisManager.createScreenshotBatches` (15-minute target, 2-minute gap, the
  unfinished trailing batch is held back)
- the batch gates: shorter than 5 minutes is skipped; `IdleBatchClassifier.assess` is called and
  an idle batch gets the fixed Idle card (without Dayflow's extension of a previous Idle card)
- the reads `LLMService.processBatch` makes: observations and cards overlapping
  `[batchEnd - 45 min, batchEnd)`, System cards left out of the prompt
- `replaceTimelineCardsInRange`: overlapping cards are replaced, clock strings resolve to the day
  nearest the window midpoint, unparsable times are dropped
- the error card a failed batch leaves behind

Not covered: Dayflow's HTTP transport (`OllamaProvider.callChatAPI`), provider fallback, the
automatic catch-up of failed batches, and concurrency. Dayflow dispatches the batches of one
analysis tick together (transcriptions may overlap; card requests are serialized). The harness
runs each batch to the end before starting the next, so the second card request always sees the
first batch's cards and observations.

## Modes

`dry` (default, and every push run): the closure records each request and answers with the
smallest JSON the validators accept, so the whole pipeline runs without network. Every output
file says `"mode": "dry"` and `"real_model_output": false`, and the texts start with `[dry-run]`.
A dry run checks that the build, the frame loading and the batch driver work. Its cards say
nothing about model behaviour.

`live`: the closure sends each request to the Anthropic Messages API (`claude-haiku-5-5` by
default). Dayflow's OpenAI-style request is converted field by field: the system message becomes
`system`, text parts become text blocks, `data:image/jpeg;base64,...` URLs become base64 image
blocks, and `max_tokens` (8000) and `temperature` (0.7) are passed on. The harness adds
`"output_config": {"effort": "low"}` to match Daytrace's default; Dayflow itself would not send
an effort field to a non-OpenRouter endpoint. If the API refuses `temperature`, the harness drops
it for the rest of the run and records that in `run.json` under `notes`.

## Frames

`frames/` holds 180 screenshots of mock pages (a code editor showing Daytrace source files, a
GitHub-style issue page and a video page), rendered from HTML and downscaled from 1920x1080 to
1280x720 at JPEG quality 85. Dayflow re-encodes to that size and quality anyway. No real screen
content is in this folder; nothing private may be added to it, because the repository is public.

`manifest.json` lists the frames 10 seconds apart. The last frame is listed two more times,
as Daytrace's replay test does, so the second 15-minute batch closes: 182 timestamps, two batches
of 91 frames. `idle_s` is 1 for every frame, the value Daytrace's replay test stores. The harness
places the frames so the last one is 10 seconds before the run started (Daytrace's replay test
does the same); set `base_ts` to pin the clock instead.

## Running it

A push to `feat/harness` that touches `reference/` or the workflow starts a dry run. Other runs:

```sh
gh workflow run dayflow-reference.yml -R ArneVDA-AP/daytrace --ref feat/harness -f mode=dry
# live: needs the repository secret ANTHROPIC_API_KEY
gh secret set ANTHROPIC_API_KEY -R ArneVDA-AP/daytrace < key-file
gh workflow run dayflow-reference.yml -R ArneVDA-AP/daytrace --ref feat/harness -f mode=live
gh secret delete ANTHROPIC_API_KEY -R ArneVDA-AP/daytrace   # after the run
```

Inputs: `mode`, `model`, `effort` (`none` leaves it out), `tz` (time zone for the clock strings,
default `Europe/Brussels`), `base_ts` (Unix time of the first frame).

The job selects the newest Xcode on the runner, checks out Dayflow at the pinned commit, copies
the test in, applies `build_fix.py`, writes
`defaults write teleportlabs.com.Dayflow analyticsOptIn -bool false`, and runs

```sh
xcodebuild test -project dayflow/Dayflow/Dayflow.xcodeproj -scheme Dayflow \
  -destination 'platform=macOS,arch=arm64' -only-testing:DayflowTests/ReplayHarnessTests \
  CODE_SIGN_IDENTITY=- CODE_SIGN_STYLE=Manual DEVELOPMENT_TEAM=
```

Settings reach the test as `TEST_RUNNER_HARNESS_*` environment variables (xcodebuild strips the
prefix). Analytics and Sentry stay off in any case: the public build has no PostHog key and no
Sentry DSN.

The build fix: Xcode 26.3, the newest Xcode on `macos-15`, stops on one long string
concatenation in `Core/Flow/FlowSessionTimeline.swift` ("unable to type-check this expression in
reasonable time"), also with raised `-solver-scope-threshold` and `-solver-memory-threshold`.
That function writes prompt JSON for Dayflow's Flow feature, which the harness does not run.
`build_fix.py` rewrites the expression as separate statements that build the same string, and
fails if the original text is not there, so a new pin cannot be patched by accident. No analysis
code is edited.

The API key goes only to live runs. Before anything is printed or uploaded, a live run searches
the output and the build log for the key and removes both if it is found. Artifacts are kept for
3 days:

```sh
gh run download <run-id> -R ArneVDA-AP/daytrace -D out/
```

The JSON files are also printed in the job log between `=====BEGIN` and `=====END` lines. Where
artifact storage is not reachable (it is served from a separate blob host), rebuild them from the
log:

```sh
gh run view <run-id> -R ArneVDA-AP/daytrace --log > run.log
python3 reference/fetch_log_output.py run.log out/harness-out
```

## Output

| File | Content |
|---|---|
| `run.json` | mode, model, effort, Dayflow commit, time zone, first-frame time, request count, token usage, cost estimate |
| `batches.json` | each batch: frames, time range, status, request count |
| `observations.json` | every observation with Unix times, clock strings and offsets from the first frame |
| `cards.json` | `final`: the timeline after the last batch; `per_batch`: what each card request returned |
| `requests.json` | every request as Dayflow built it, images replaced by their size and dimensions; the converted Anthropic request (`anthropic_request` in live mode, `anthropic_request_preview` in dry mode), the response text, and in live mode the stop reason and usage |
| `xcodebuild.log` | full build and test log |

## Comparing with Daytrace

Run Daytrace's replay test on the same frames with JSON output (this calls the model):

```sh
cd src-tauri
DAYTRACE_API_KEY=... DAYTRACE_REPLAY_DIR=../reference/frames DAYTRACE_PIPELINE=cloud \
DAYTRACE_REASONING=low DAYTRACE_REPLAY_JSON=/tmp/daytrace-replay.json \
  cargo test --test replay -- --ignored --nocapture
python3 ../reference/compare.py out/<artifact>/harness-out /tmp/daytrace-replay.json
```

`compare.py` prints, with times as offsets from the first frame so runs at different clock
times line up:

- run settings for both sides (a dry harness run is labelled as such)
- batches and their status
- observation counts per batch, mean length and span, and every observation
- cards side by side (offsets, clock strings, category, title), minute coverage, how many covered
  minutes agree on the category, the card boundaries, and each card's summary, subcategory, sites,
  distractions and detailed-summary length
- for the first transcription and the first card request: whether the system message, the
  prompt (for cards, the instructions after `</observations>`), the image count and the image
  labels match, with a line diff where they do not, and the request parameters each side sent
- requests and token usage per operation, with a cost estimate at Claude Haiku 5.5 list prices
