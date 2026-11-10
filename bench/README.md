# Benchmark

Real-world, mostly AI-written repos (`repos.txt`, pinned by commit) with hand labels
(`labels.jsonl`) of whether each sampled finding is real slop or noise.

```sh
cargo build --release -p monti-cli
python3 bench/run.py              # precision per rule; clones into bench/.cache on first run
python3 bench/run.py --sample 15  # unlabeled findings with code context, to label
```

A PR that changes a rule should show the table before and after. Lower precision, or a non-zero
`lost real`, needs a reason.

## Results (36 repos, 176 labels)

Labels came in two rounds of random samples. Round 1 (12 per rule) drove the first set of fixes.
Round 2 (8 per rule) was drawn and labeled *after* those fixes and *before* the second set, so its
precision is the honest estimate of how the rules do on code they were not tuned on:

| rule | round 2 precision (unseen) | precision now, all labels | findings before → after |
|---|---|---|---|
| restating-comment | 6/6 | 100% | 56 → 9 |
| type-escape | 6/6 | 100% | 24 → 10 |
| broad-except | 8/8 | 100% | 82 → 13 |
| commented-error | 6/8 | 82% | 52 → 12 |
| swallowed-error | 6/8 | 64% | 52 → 45 |
| duplicate-function | 5/8 | 77% | 2465 → 1466 |
| pass-through-wrapper | 3/8 | 88% | 66 → 16 |
| unused-parameter | 3/8 | 78% | 54 → 19 |
| comment-flood | 2/8 | 100% (1 label) | 460 → 2 |

What the labels taught (each is a fix in the rules or detectors):

- An empty handler for a *named* exception is a decision; only catch-alls are swallowed errors.
- A handler comment that gives a reason ("private browsing: not remembered") is deliberate;
  "ignore" / "silent fail" is a shrug.
- `except Exception` that binds and logs the error is reporting it, not hiding it.
- Wrappers that add defaults, decorated functions, object/dict property values, JS member calls
  (`this`), and C++ methods defined outside their class are not pass-through slop.
- Full-sentence comments explain why; only short narration (`% Create figure`) floods.
  Commented-out code is not narration.
- Small functions of the same shape with different names are not copies; small copy-pastes keep
  their names. N copies of a helper are N findings, not N × (N − 1).
- `any` in tests poking at raw fixture JSON, and test doubles matching a real signature, are fine.
- A parameter list shared by 3+ functions is an imposed signature (a dispatch table).

Known misses (labeled real, not reported): a 43-node function re-implemented with renamed
variables; a C++ parameter ignored in a function sharing its signature with two others; one
function with 3 narration comments, under the ratio. Identical lines followed by identical lines in
one file share a fingerprint, so one label covers both.
