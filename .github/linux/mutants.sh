#!/usr/bin/env bash
set -u

export SCOOCH_NEED_BUS=1
cargo test -p scooch --no-run >/dev/null 2>&1

echo "== each test before any mutation"
cargo test -p scooch -- --exact \
    platform::linux::tests::a_tick_the_desktop_did_not_answer_is_tried_again_with_no_wake_and_the_waits_end_with_a_tick_that_works \
    platform::linux::screens::tests::an_x_server_that_never_answers_ends_in_plain_words_at_the_limit 2>&1 | grep -E "^test |test result"

for patch in .github/linux/mutants/*.patch; do
    tests=$(grep '^# test ' "$patch" | sed 's/^# test //')
    echo "== $(basename "$patch")"
    git apply "$patch" || { echo "the mutation did not apply"; continue; }
    for test in $tests; do
        out=$(cargo test -p scooch -- --exact "$test" 2>&1)
        code=$?
        echo "$test exit $code"
        echo "$out" | grep -E -A2 "panicked|^test |test result" | head -n 8
    done
    git apply -R "$patch"
done
