#!/usr/bin/env bash
set -u

export SCOOCH_NEED_BUS=1
cargo test -p scooch --no-run >/dev/null 2>&1

echo "== each test before any mutation"
cargo test -p scooch -- --exact \
    platform::linux::tests::a_peer_that_takes_the_call_and_never_answers_ends_in_plain_words_at_the_limit \
    platform::linux::tests::a_tick_held_by_a_call_nobody_answers_comes_back_and_the_next_tick_draws \
    platform::linux::tests::a_program_that_never_ends_is_stopped_at_the_limit_in_plain_words \
    platform::linux::tests::a_program_that_ends_gives_its_output_and_status \
    platform::linux::screens::tests::a_compositor_that_never_answers_ends_in_plain_words_at_the_limit 2>&1 | grep -E "^test |test result"

for patch in .github/linux/mutants/*.patch; do
    tests=$(grep '^# test ' "$patch" | sed 's/^# test //')
    echo "== $(basename "$patch")"
    git apply "$patch" || { echo "the mutation did not apply"; continue; }
    for test in $tests; do
        out=$(cargo test -p scooch -- --exact "$test" 2>&1)
        code=$?
        echo "$test exit $code"
        echo "$out" | grep -E "panicked|^test |test result" | head -n 6
    done
    git apply -R "$patch"
done
