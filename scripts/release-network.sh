#!/usr/bin/env bash
# Shared bounded retries for explicitly classified transient network failures.

release_network_transient() {
    local stdout=$1 stderr=$2
    if grep -Eiq '^HTTP/[0-9.]+ (400|401|403|404|409|422)([[:space:]]|$)' "$stdout" "$stderr"; then
        return 1
    fi
    grep -Eiq '(^HTTP/[0-9.]+ (408|429|500|502|503|504)([[:space:]]|$)|HTTP (408|429|500|502|503|504)([^0-9]|$)|unexpected EOF|connection (reset|refused|closed|timed out|was lost)|TLS handshake timeout|timed? out|timeout|deadlineExceeded|abortedUpload|temporary failure|temporarily unavailable|could not resolve|failed to connect|network is unreachable|network connection was lost)' "$stdout" "$stderr"
}

release_retry_wait() {
    local attempt=$1 stdout=$2 stderr=$3
    if (( attempt >= 3 )) || ! release_network_transient "$stdout" "$stderr"; then
        cat "$stdout"
        cat "$stderr" >&2
        return 1
    fi
    cat "$stderr" >&2
    printf 'Temporary network failure; retrying (%d/3) after %ds.\n' "$((attempt + 1))" "$attempt" >&2
    sleep "$attempt"
}

# Use only for read-only operations or explicitly idempotent operations on the
# same artifact. Each attempt writes fresh output and error files.
release_retry_read() (
    local dir attempt status
    dir=$(mktemp -d) || return
    trap 'rm -rf "$dir"' EXIT
    for attempt in 1 2 3; do
        : >"$dir/out"
        : >"$dir/err"
        if "$@" >"$dir/out" 2>"$dir/err"; then
            cat "$dir/out"
            cat "$dir/err" >&2
            return 0
        else
            status=$?
        fi
        if ! release_retry_wait "$attempt" "$dir/out" "$dir/err"; then
            return "$status"
        fi
    done
)
