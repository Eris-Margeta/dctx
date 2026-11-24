# Windows A/B latency verdict and its evidence

The 23 November 2026 maintenance snapshot reports three green hosted Windows comparisons against `"v1.3.2"` for `"ae4944fa"`. It places the injected slowdown on local Linux. **No checked-in acceptance proves that the hosted measurement step caught that slowdown, failed the job, and uploaded the corresponding FAIL report.** The checked-in test does prove that a synthetic regression survives the Python path through report writing and return code. The distinction matters when using the green badge as release evidence. See `scripts/test_win_latency_ab.py:73`, `.github/workflows/win-latency.yml:63` and `README.md:72`.

Sources and tests below are from `"fb55bad8557d"`. The dated run claims are attributed to the snapshot. This audit read the workflow and exercised Python offline with fixed inputs. Actual Windows timings, run logs and artifact contents were unmeasured.

## From two samples to one job outcome

1. **Select and identify the inputs.** The Windows 2022 job checks out the event revision, builds the locked release CLI, resolves the latest release tag once, downloads its Windows archive and checksum list, verifies the archive against that list, and checks for the extracted executable. It passes two paths and the tag to Python. The baseline is the release selected *at run time*. See `.github/workflows/win-latency.yml:25`, `.github/workflows/win-latency.yml:43`, `.github/workflows/win-latency.yml:50` and `.github/workflows/win-latency.yml:68`.
2. **Pair the runs.** Both arms query the same deterministic 1,500-file fixture with one matching symbol. File times are set ten seconds before fixture creation. Zero-based round 0 runs baseline then candidate, round 1 reverses them, and the order alternates for seven rounds. Baseline goes first four times. Each arm gets its own cache directory per round. Each cold query starts a new process with a fresh application cache. A separate warm cache receives one excluded warmup and 30 timed processes. “Cold” describes the application cache. OS cache state is uncontrolled. The monotonic clock spans process launch through completion. This gives 61 invocations per arm per round, 854 total. See `scripts/win_latency.py:41`, `scripts/win_latency.py:68`, `scripts/win_latency.py:81` and `scripts/win_latency_ab.py:37`.
3. **Reduce the samples.** Within a 30-sample mode, sort and select ranks 15 and 29 for p50 and p95. The A/B row retains cold p50, cold p95 and warm p95. Each arm's seven cold p95s are independently sorted and the fourth value becomes its gate statistic. Warm p95 is displayed. In a five-round probe, baseline `[100,100,400,400,400]` and candidate `[200,200,800,410,410]` gave median cold p95s of 400 and 410 ms and a passing ratio of 1.025. The median of the five paired ratios was 2.0. This illustrates the statistic specified in `CONTRIBUTING.md:103` and implemented at `scripts/win_latency.py:28`, `scripts/win_latency_ab.py:41` and `scripts/win_latency_ab.py:51`.
4. **Decide and publish.** Either a ratio strictly above 1.2 or a candidate median strictly above 1,000 ms adds a failure reason. Both can apply. Above 500 ms sets a separate target flag and emits an Actions warning. Python writes JSON before printing the verdict, appends the Markdown summary and returns 1 for FAIL or 0 for PASS. The measurement step ends with that command and has no failure override. The upload step runs after measurement success or failure unless cancelled. It uploads `target/win-latency/report.json` as `"win-latency"` for 14 days and errors on a missing file. Inferred workflow consequence: a genuine gate failure leaves the job red even if upload succeeds. A skipped measurement skips upload. See `scripts/win_latency_ab.py:62`, `scripts/win_latency_ab.py:140`, `scripts/win_latency_ab.py:151` and `.github/workflows/win-latency.yml:67` through `.github/workflows/win-latency.yml:79`.

**Observed end-to-end synthetic pair.** A one-file replay used the production A/B entry point and sampler with fake process responses and nanosecond clock ticks. In each round, baseline cold durations descended from 401 to 372 ms plus that round's offset. Candidate added 80,000,001 ns to each corresponding duration. Offsets in round order were `[-3,0,2,-2,3,1,-1]` ms. In the zero-offset round the baseline p50/p95 were 386/400 ms and the candidate p50/p95 were 466.000001/480.000001 ms. The 9,000 ms warmup contributed to neither distribution. All timed warm samples were 20 ms. All 854 calls followed the expected order. The JSON median cold p95s were 400 and 480.000001 ms, the verdict was FAIL, and the entry point returned 1. Distinct but stable stdout for the two arms was accepted, as explicitly covered by `scripts/test_win_latency.py:120`. The byte reference begins anew for each binary measurement at `scripts/win_latency.py:63`. A passing comparison establishes within-measurement repeatability and the presence of the fixture symbol. It cannot establish cross-version answer equivalence.

## Findings

### 1. Hosted negative-control evidence is open

**Observed, high confidence in the coverage boundary.** The harness test supplies 450/450 and 450/700 ms via a mock, checks return codes 0/1, reads each JSON verdict and inspects the summary. The workflow runs those unit tests, then measures the real candidate against a real release. Its measurement command has no injected control. The action uploads the file without parsing the verdict. See `scripts/test_win_latency_ab.py:74`, `scripts/test_win_latency_ab.py:85`, `.github/workflows/win-latency.yml:59` and `.github/workflows/win-latency.yml:72`.

**User consequence.** A reviewer can cite the three reported green runs for the ordinary comparison and the reported Linux control for local detection. A hosted control requires a failing measurement step for the injected candidate, a failed job and the matching uploaded JSON. No such receipt was identified in the checked-in evidence. Confidence in the historical counts is limited to the dated account.

### 2. The detached report cannot establish when or which binaries ran

**Observed, high confidence.** Python resolves both executables, then records a platform string, fixture size, requested counts, a caller-supplied baseline label and the statistic. The report has round indices and order, but no UTC measurement timestamp, run ID/attempt, checkout SHA, candidate label or digest of either executable. The age assigned to fixture files and the elapsed clock are unsuitable as run timestamps. The download checksum in the workflow binds the archive to the release's checksum list. It does not travel with the report. The full per-mode sample vectors from the sampler are discarded when constructing A/B rows. See `scripts/win_latency_ab.py:119`, `scripts/win_latency_ab.py:127`, `scripts/win_latency.py:94` and `scripts/win_latency_ab.py:41`.

**Observed probe and consequence.** Supplying the same resolved executable for both arms with a caller-chosen tag produced PASS under mocked measurements. This demonstrates the limit of trusting a label. The workflow itself supplies distinct build and download paths. A reviewer with only the artifact can recompute the medians and ratio from its rows, but cannot authenticate the comparison, date it, or recompute the nearest-rank values from the underlying samples.

### 3. Exact 20% equality can turn red after conversion to milliseconds

Five identical rows per arm, with warm p95 fixed at 3,000 ms, produced the following results directly from `scripts/win_latency_ab.py:60`. Inputs are cold p95s in ms.

| Baseline | Candidate | Verdict and reasons | Target missed |
| ---: | ---: | --- | --- |
| 400 | 480 | PASS | no |
| 400 | 480.000001 | FAIL, relative | no |
| 400.000010 | 480.000012 | FAIL, relative | no |
| 1,000 | 1,000 | PASS | yes |
| 1,000 | 1,000.000001 | FAIL, ceiling | yes |
| 500 | 500 | PASS | no |
| 500 | 500.000001 | PASS | yes |
| 400 | 1,001 | FAIL, relative and ceiling | yes |

**Observed, high confidence.** In the third row, five times 480,000,012 ns equals six times 400,000,010 ns exactly. Converting elapsed nanoseconds to floating-point milliseconds at `scripts/win_latency.py:71` and dividing yields 1.2000000000000002. The published strict inequality should pass that pair. The existing equality test uses 400/480, which does pass. Both a genuine one-nanosecond excess and the false failure can print a 20.0% slowdown because the explanation rounds to one decimal place. See `scripts/test_win_latency_ab.py:63`, `scripts/win_latency_ab.py:62` and `scripts/win_latency_ab.py:85`. **User consequence.** An exactly-at-limit build can fail, and the summary alone cannot resolve a close call. Frequency on hosted runners is unmeasured.

### 4. A partial or invalid pair can yield a misleading verdict

**Observed, high confidence for direct helper calls.** The sampler rejects a request for four rounds. A measurement error on the second arm aborts before a new report is written. In contrast, the verdict helper takes the median of whatever lists it receives. Five baseline rows at 400 ms and a single candidate row numbered 3 at 410 ms yielded PASS. Rendering a requested count of five displayed one paired row labelled 0. It zips the lists by position and uses the baseline index. An empty baseline raised `StatisticsError` and an absent baseline key raised `KeyError`. See `scripts/test_win_latency_ab.py:37`, `scripts/win_latency_ab.py:40`, `scripts/win_latency_ab.py:51` and `scripts/win_latency_ab.py:90`.

A zero or negative baseline median with a positive candidate produced FAIL with an infinite ratio. The default JSON encoder writes the non-standard token `Infinity`, which strict JSON rejects. A positive-infinite baseline with a finite 400 ms candidate gave ratio zero and PASS. A candidate median of NaN also passed both comparisons. See `scripts/win_latency_ab.py:60`, `scripts/win_latency_ab.py:65` and `scripts/win_latency_ab.py:141`. **User consequence.** A consumer reusing the helpers can publish a green result for uncomparable data or receive an unreadable artifact. These malformed rows were supplied directly. The normal CLI collects both arms synchronously and has no input-report or resume mode. There is no evidence of a partial or nonfinite hosted sample.

### 5. A failed preflight can leave an older artifact eligible for upload

**Observed, high confidence for local control flow.** A disposable output containing a sentinel survived a missing-executable error (exit 2). Executable lookup precedes output removal at `scripts/win_latency_ab.py:121` and `scripts/win_latency_ab.py:126`. With lookup successful, a subsequent synthetic second-arm error removed the sentinel and left no report. **Inferred user consequence.** When reusing an output location, a preflight failure can leave an old verdict at the advertised path. The upload condition checks step outcome and file existence, so it could upload that older file if one was already present. A clean runner with no report instead gets the configured missing-file error. No stale hosted upload was observed. See `.github/workflows/win-latency.yml:73`.

### 6. Release-tag interpolation can execute shell commands, and labels can alter the displayed verdict

**Observed in the workflow, high confidence.** The fetch step writes the selected release tag to its Actions output at `.github/workflows/win-latency.yml:58`. At `.github/workflows/win-latency.yml:68`, Actions substitutes that output directly into the double-quoted `--baseline-label` argument while constructing the script. Bash parses the resulting source before Python receives an argument. An offline `git check-ref-format` accepted the tag shown here as a JSON string, `"v1\"rc1"`. Its double quote changes the quoting in the generated command.

**Inferred user consequence.** With that tag, the unmatched quote can stop the measurement command before Python starts. No fresh verdict is written. On a clean runner the upload step then encounters a missing file at `.github/workflows/win-latency.yml:78`. A red job in this case supplies no latency comparison. The quoted-tag probe demonstrates only a parse failure. If someone controls the release tag selected at `.github/workflows/win-latency.yml:50` and the fetch succeeds, shell syntax in that tag can execute commands with the measurement step’s runner permissions. Those commands could alter the report before upload, compromising the verdict a reviewer relies on. Confidence in this conditional risk is high because Actions inserts the tag into Bash source before parsing. The reviewed evidence establishes neither public control of the selected tag nor exploitation on a hosted runner.

**Observed at the Python entry point, high confidence.** The synthetic CLI label, JSON-encoded here as `"vX\n\u001b\""`, round-tripped safely in JSON. The same input created two stdout lines and inserted a raw escape character in both stdout and Markdown. The free-form label enters the heading through `scripts/win_latency_ab.py:85`, stdout at `scripts/win_latency_ab.py:143` and the job summary at `scripts/win_latency_ab.py:150`. Readers or line-oriented consumers can misread which text belongs to the verdict. This fixture exercises a CLI caller. Git forbids newline and ESC in ref names.

A second synthetic label, `"**rc** [notes](#details)"`, stays on one line. A local CommonMark parser reads it as bold text and a link in the summary heading at `scripts/win_latency_ab.py:99`. A reviewer sees formatting in place of the literal identifier. The following bounded characterization uses markdown-it-py and keeps both fixture values in memory.

```sh
python3 -B - <<'PY'
import json
from markdown_it import MarkdownIt
from scripts import win_latency_ab as ab

label = 'vX\n\x1b"'
rows = [dict(round=i, cold_p95_ms=400, warm_p95_ms=20) for i in range(5)]
pair = dict(baseline=rows, candidate=rows)
report = dict(baseline_label=label, rounds=pair, rounds_count=5, runs_per_mode=30,
              files=1, platform="fixture", gate=ab.verdict(pair, .2, 1000, 500))
line, summary = ab.render(report)
assert json.loads(json.dumps(report))["baseline_label"] == label
assert len(line.splitlines()) == 2 and label in line and label in summary

markup = "**rc** [notes](#details)"
_, summary = ab.render(dict(report, baseline_label=markup))
heading, inline = MarkdownIt().parse(summary)[:2]
assert heading.tag == "h3" and inline.type == "inline"
assert {"strong_open", "link_open"} <= {token.type for token in inline.children}
PY
```

### 7. Two stable answers can identify different files and still pass

**Observed offline, high confidence.** The fixture places its only match in `"group_0000/item_0000.rs"` at `scripts/win_latency.py:43`. In a seven-round replay with all timed calls fixed at 400 ms, the baseline returned that file’s symbol ID. The candidate consistently returned the same symbol name under `"group_0000/item_0001.rs"`. Both passed the substring check at `scripts/win_latency.py:72` and their separate byte checks at `scripts/win_latency.py:76`. The A/B gate returned PASS. The test at `scripts/test_win_latency.py:120` explicitly accepts distinct stable output per binary. **User consequence.** A green comparison can credit the candidate for the latency of a wrong answer. Whether any hosted comparison returned different matches is unmeasured.

## Follow-up cards

Existing tests cover normal alternation and fresh caches at `scripts/test_win_latency_ab.py:27`, runner noise and outliers at `scripts/test_win_latency_ab.py:48`, and ordinary verdict and return-code agreement at `scripts/test_win_latency_ab.py:73`.

1. **Prove the injected slowdown changed the hosted verdict (finding 1).** Write seams are `.github/workflows/win-latency.yml`, `scripts/win_latency.py`, `scripts/win_latency_ab.py` and the proposed `scripts/check_win_latency_control.py`. In an opt-in Windows run, record each timed sample’s elapsed nanoseconds before adjustment and the value actually passed to the percentile calculation. Apply a configured positive increment only to candidate cold samples. Keep the usual measurement, verdict, exit and artifact upload steps.

   For a separately authorized hosted run, accept with `python3 scripts/check_win_latency_control.py job.json report.json` using that run’s job metadata and downloaded artifact. Match the run ID, attempt and candidate revision. Require seven alternating round pairs with 30 cold and 30 warm samples per arm per round. Match samples by arm, round, mode and index. For every candidate cold sample require the actual gate input minus its recorded elapsed time to equal the requested positive increment. Require zero difference for every baseline and warm sample. Recompute the round p95s, medians and verdict from the actual gate inputs at the workflow’s 20% limit and 1,000 ms ceiling. Require agreement with the uploaded report, FAIL for the relative limit alone, a failed measurement step and job, and a successful upload. Replay those **same samples** with only the verified increment removed. Require PASS on both limits.

   A bounded checker fixture can use 400 ms baseline and 410 ms raw candidate cold samples in every round. Adding 80 ms yields 490 ms and a relative FAIL. Removing it restores the 410 ms PASS. Require the checker to reject a control marked enabled with a raw and gated candidate of 490 ms (no increment applied), and a raw candidate of 490 ms raised to 570 ms (already failing before injection). An ordinary green run, an unrelated job failure and a swapped artifact must also fail acceptance. These examples specify the receipt check. The hosted control remains unmeasured.
2. **Make a detached comparison traceable and replayable (finding 2).** Write seams: `scripts/win_latency_ab.py`, `.github/workflows/win-latency.yml`, `scripts/test_win_latency_ab.py`. Proposed check: `python3 -B -m unittest scripts.test_win_latency_ab.MainTests.test_report_provenance`. Use two distinct disposable binary contents, a fixed UTC clock, run ID/attempt and checkout revision. Verify both hashes against the input bytes, the labels and clock against the fixture, and all p95s against retained sample vectors. Distinguish an intentional self-comparison by its matching hashes.
3. **Decide strict thresholds at sample precision (finding 3).** Write seams: `scripts/win_latency.py`, `scripts/win_latency_ab.py`, `scripts/test_win_latency_ab.py`. Proposed check: `python3 -B -m unittest scripts.test_win_latency_ab.GateTests.test_nanosecond_boundaries`. With baseline 400,000,010 ns require PASS at 480,000,012 ns and FAIL one ns higher. Assert ceiling equality/one ns over and warning-only target equality/one ns over independently. Require the failure detail to distinguish each just-over value from its limit.
4. **Validate paired rounds before computing or rendering a verdict (finding 4).** Write seams: `scripts/win_latency_ab.py` and `scripts/test_win_latency_ab.py`. Proposed check: `python3 -B -m unittest scripts.test_win_latency_ab.GateTests.test_invalid_pairs`. Exercise absent/empty arms, truncated, duplicate and misaligned round IDs, nonpositive baseline and nonfinite times. Require a controlled invalid-measurement outcome, no PASS, and strict JSON for any emitted report. Keep the complete positive pair as a passing control.
5. **Prevent reuse of a previous verdict after preflight failure (finding 5).** Write seams: `scripts/win_latency_ab.py`, `scripts/test_win_latency_ab.py`, and `.github/workflows/win-latency.yml` if the artifact path is scoped per attempt. Proposed check: `python3 -B -m unittest scripts.test_win_latency_ab.MainTests.test_failed_attempt_artifact`. Seed a prior PASS in a disposable directory, fail executable lookup and then separately fail during sampling. Each attempt must exit nonzero and expose no prior PASS at its upload path. A completed threshold failure must still expose its own FAIL JSON.
6. **Pass the release tag literally to Bash and escape the displayed label (finding 6).** Write seams are `.github/workflows/win-latency.yml`, `scripts/win_latency_ab.py` and `scripts/test_win_latency_ab.py`. Bind the tag output to the measurement step's `BASELINE_TAG` environment variable and use `--baseline-label "$BASELINE_TAG"` in the Bash command. For the rendering half, run the proposed `python3 -B -m unittest scripts.test_win_latency_ab.MainTests.test_escaped_baseline_label` through the CLI with each of the two labels above. Capture stdout, read the written job summary and parse the written JSON. In each case require one stdout verdict line, one level-three summary heading, and `report["baseline_label"] == input_label` after JSON decoding. For the control-character case require visible escapes for the newline, ESC and quote on both display surfaces. For the markup case parse the summary as Markdown and require its heading’s rendered text to contain the exact literal `**rc** [notes](#details)`, including the punctuation, with no strong-emphasis or link node for the label. Counting headings alone would accept the current misrendering.

   For workflow acceptance, run the following offline from the repository root with PyYAML, Git and Bash. It reads the actual measurement step, verifies where Actions places the tag, then runs the step body with a Bash function standing in for Python. NUL-delimited arguments preserve the exact bytes that the measurement script would receive. The checked-in step failed the environment-binding assertion before Bash ran the step. An in-memory copy with the proposed binding passed and delivered the quoted tag as one argument.

   ```sh
   python3 -B - <<'PY'
   import os
   from pathlib import Path
   import subprocess
   import yaml

   workflow = yaml.safe_load(Path(".github/workflows/win-latency.yml").read_text())
   job = workflow["jobs"]["latency"]
   step, = (s for s in job["steps"] if s.get("id") == "measure")
   tag = 'v1"rc1'
   ref = subprocess.run(["git", "check-ref-format", "refs/tags/" + tag], capture_output=True)
   assert ref.returncode == 0
   assert step.get("env", {}).get("BASELINE_TAG") == "${{ steps.baseline.outputs.tag }}"
   body = step["run"]
   assert "${{" not in body and '--baseline-label "$BASELINE_TAG"' in body
   assert step.get("shell", job["defaults"]["run"]["shell"]) == "bash"
   recorder = r'''python() { printf '%s\0' "$@"; }'''
   result = subprocess.run(["bash", "-e", "-o", "pipefail", "-c", recorder + "\n" + body],
                           env={"PATH": os.defpath, "RUNNER_TEMP": ".", "BASELINE_TAG": tag},
                           capture_output=True)
   assert result.returncode == 0
   args = result.stdout.split(b"\0")
   assert args[0] == b"scripts/win_latency_ab.py" and args.count(b"--baseline-label") == 1
   i = args.index(b"--baseline-label")
   assert args[i + 1:i + 3] == [tag.encode(), b"--candidate"]
   PY
   ```

7. **Require the same fixture match from both binaries (finding 7).** Write seams are `scripts/win_latency.py`, `scripts/win_latency_ab.py`, `scripts/test_win_latency.py` and `scripts/test_win_latency_ab.py`. Run the proposed `python3 -B -m unittest scripts.test_win_latency_ab.MainTests.test_cross_version_fixture_match`. Exercise the A/B entry point with fixed passing timings and stable subprocess output. In every round require each arm’s parsed matches to contain exactly the fixture’s symbol ID `"s:group_0000/item_0000.rs#win_latency_probe"` with kind `"fn"`. Use different display spacing for two equivalent rows and require exit 0 and PASS in the written JSON report. Then change only the candidate’s path to `"group_0000/item_0001.rs"`, leaving the symbol name, kind and timings intact. Require a nonzero exit, a diagnostic identifying the answer mismatch and no PASS report. Keep the within-binary byte-repeatability test at `scripts/test_win_latency.py:106` and update the permissive test at `scripts/test_win_latency.py:120` to use semantically equivalent renderings.
