# Compatibility Plan

Behavioral reference: installed `9router@0.5.75`. Differential harness: `scripts/test-differential.sh` boots original gateway (if installed) + `9router-rs serve` on adjacent ports, replays `fixtures/*` + mock upstream, normalizes timestamps/ids/tokens, diffs status/schema/stream events. Offline default: mock upstream in `tests/`.
