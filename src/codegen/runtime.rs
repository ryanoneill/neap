//! Embedded zsh runtime library for Neap
//!
//! This module contains the runtime functions that are embedded in
//! generated zsh scripts. All Neap values are represented as JSON.

/// The complete zsh runtime library as a string.
/// This is embedded at the top of every generated script.
pub const RUNTIME: &str = r#"
# ══════════════════════════════════════════════════════════════════════════════
# Neap Runtime Library for Zsh
# Generated code - do not edit
# Requires: jq
# ══════════════════════════════════════════════════════════════════════════════

# Check for required dependencies
if ! command -v jq &> /dev/null; then
    echo "Error: jq is required but not installed" >&2
    exit 1
fi

# ─────────────────────────────────────────────────────────────────────────────
# Function Calls and Closures
# ─────────────────────────────────────────────────────────────────────────────

# Call a function or apply a closure
# Usage: __neap_call <fn_or_closure> <args...>
__neap_call() {
    local fn_or_closure=$1
    shift

    # Check if it's a closure (JSON object with _fn field)
    local fn_name
    fn_name=$(echo "$fn_or_closure" | jq -r '._fn // empty' 2>/dev/null)

    if [[ -n "$fn_name" ]]; then
        # It's a closure - extract captured args and combine with new args
        local arity captured_json
        arity=$(echo "$fn_or_closure" | jq -r '._arity')
        captured_json=$(echo "$fn_or_closure" | jq -c '._args')

        # Convert captured args to array
        local -a captured_args
        while IFS= read -r arg; do
            captured_args+=("$arg")
        done < <(echo "$captured_json" | jq -r '.[]')

        # Combine captured args with new args
        local -a all_args=("${captured_args[@]}" "$@")
        local total_args=${#all_args[@]}

        if [[ $total_args -lt $arity ]]; then
            # Still partial - create new closure
            __neap_partial "$fn_name" "$arity" "${all_args[@]}"
        else
            # Full application - call the function
            "$fn_name" "${all_args[@]}"
        fi
    else
        # Direct function call
        "$fn_or_closure" "$@"
    fi
}

# Create a partial application (closure)
# Usage: __neap_partial <fn_name> <arity> <captured_args...>
__neap_partial() {
    local fn=$1 arity=$2
    shift 2

    # Build JSON array of captured args
    local args_json="[]"
    for arg in "$@"; do
        args_json=$(echo "$args_json" | jq --arg a "$arg" '. + [$a]')
    done

    jq -nc --arg fn "$fn" --argjson arity "$arity" --argjson args "$args_json" \
        '{_fn: $fn, _arity: $arity, _args: $args}'
}

# ─────────────────────────────────────────────────────────────────────────────
# JSON Helpers
# ─────────────────────────────────────────────────────────────────────────────

# Get a field from a JSON value
# Usage: __neap_get <json> <jq_path>
__neap_get() {
    echo "$1" | jq -c "$2"
}

# Get the tag of an ADT value
# Usage: __neap_tag <json>
__neap_tag() {
    echo "$1" | jq -r '._tag'
}

# ─────────────────────────────────────────────────────────────────────────────
# Integer Arithmetic
# ─────────────────────────────────────────────────────────────────────────────

__neap_add_int() { echo $(($1 + $2)); }
__neap_sub_int() { echo $(($1 - $2)); }
__neap_mul_int() { echo $(($1 * $2)); }
__neap_div_int() { echo $(($1 / $2)); }
__neap_mod_int() { echo $(($1 % $2)); }
__neap_neg_int() { echo $((-$1)); }

# ─────────────────────────────────────────────────────────────────────────────
# Float Arithmetic (using bc for precision)
# ─────────────────────────────────────────────────────────────────────────────

__neap_add_float() { echo "$1 + $2" | bc -l; }
__neap_sub_float() { echo "$1 - $2" | bc -l; }
__neap_mul_float() { echo "$1 * $2" | bc -l; }
__neap_div_float() { echo "scale=10; $1 / $2" | bc -l; }
__neap_neg_float() { echo "-($1)" | bc -l; }

# ─────────────────────────────────────────────────────────────────────────────
# Integer Comparison
# ─────────────────────────────────────────────────────────────────────────────

__neap_eq_int() { [[ $1 -eq $2 ]] && echo true || echo false; }
__neap_neq_int() { [[ $1 -ne $2 ]] && echo true || echo false; }
__neap_lt_int() { [[ $1 -lt $2 ]] && echo true || echo false; }
__neap_le_int() { [[ $1 -le $2 ]] && echo true || echo false; }
__neap_gt_int() { [[ $1 -gt $2 ]] && echo true || echo false; }
__neap_ge_int() { [[ $1 -ge $2 ]] && echo true || echo false; }

# ─────────────────────────────────────────────────────────────────────────────
# Float Comparison
# ─────────────────────────────────────────────────────────────────────────────

__neap_eq_float() { (( $(echo "$1 == $2" | bc -l) )) && echo true || echo false; }
__neap_neq_float() { (( $(echo "$1 != $2" | bc -l) )) && echo true || echo false; }
__neap_lt_float() { (( $(echo "$1 < $2" | bc -l) )) && echo true || echo false; }
__neap_le_float() { (( $(echo "$1 <= $2" | bc -l) )) && echo true || echo false; }
__neap_gt_float() { (( $(echo "$1 > $2" | bc -l) )) && echo true || echo false; }
__neap_ge_float() { (( $(echo "$1 >= $2" | bc -l) )) && echo true || echo false; }

# ─────────────────────────────────────────────────────────────────────────────
# String Comparison
# ─────────────────────────────────────────────────────────────────────────────

__neap_eq_string() { [[ $1 == "$2" ]] && echo true || echo false; }
__neap_neq_string() { [[ $1 != "$2" ]] && echo true || echo false; }
__neap_lt_string() { [[ $1 < "$2" ]] && echo true || echo false; }
__neap_le_string() { [[ ! $1 > "$2" ]] && echo true || echo false; }
__neap_gt_string() { [[ $1 > "$2" ]] && echo true || echo false; }
__neap_ge_string() { [[ ! $1 < "$2" ]] && echo true || echo false; }

# ─────────────────────────────────────────────────────────────────────────────
# Boolean Comparison
# ─────────────────────────────────────────────────────────────────────────────

__neap_eq_bool() { [[ $1 == "$2" ]] && echo true || echo false; }
__neap_neq_bool() { [[ $1 != "$2" ]] && echo true || echo false; }

# ─────────────────────────────────────────────────────────────────────────────
# Char Comparison
# ─────────────────────────────────────────────────────────────────────────────

__neap_eq_char() { [[ $1 == "$2" ]] && echo true || echo false; }
__neap_neq_char() { [[ $1 != "$2" ]] && echo true || echo false; }
__neap_lt_char() { [[ $1 < "$2" ]] && echo true || echo false; }
__neap_le_char() { [[ ! $1 > "$2" ]] && echo true || echo false; }
__neap_gt_char() { [[ $1 > "$2" ]] && echo true || echo false; }
__neap_ge_char() { [[ ! $1 < "$2" ]] && echo true || echo false; }

# ─────────────────────────────────────────────────────────────────────────────
# Logical Operations
# ─────────────────────────────────────────────────────────────────────────────

__neap_not() { [[ $1 == "true" ]] && echo false || echo true; }
__neap_and() { [[ $1 == "true" && $2 == "true" ]] && echo true || echo false; }
__neap_or() { [[ $1 == "true" || $2 == "true" ]] && echo true || echo false; }

# ─────────────────────────────────────────────────────────────────────────────
# String Operations
# ─────────────────────────────────────────────────────────────────────────────

# Concatenate two strings (both are JSON strings)
__neap_concat() {
    local s1 s2
    s1=$(echo "$1" | jq -r '.')
    s2=$(echo "$2" | jq -r '.')
    jq -n --arg s "$s1$s2" '$s'
}

# Get string length
__neap_string_length() {
    local s
    s=$(echo "$1" | jq -r '.')
    echo ${#s}
}

# Get character at index
__neap_char_at() {
    local s idx
    s=$(echo "$1" | jq -r '.')
    idx=$2
    echo "${s:$idx:1}"
}

# Get substring
__neap_substring() {
    local s start len
    s=$(echo "$1" | jq -r '.')
    start=$2
    len=$3
    jq -n --arg s "${s:$start:$len}" '$s'
}

# ─────────────────────────────────────────────────────────────────────────────
# List Operations
# ─────────────────────────────────────────────────────────────────────────────

# Cons: prepend element to list
__neap_cons() {
    local elem=$1 list=$2
    echo "$list" | jq --argjson e "$elem" '[$e] + .'
}

# Append: concatenate two lists
__neap_append() {
    local list1=$1 list2=$2
    jq -n --argjson a "$list1" --argjson b "$list2" '$a + $b'
}

# List length
__neap_list_length() {
    echo "$1" | jq 'length'
}

# ─────────────────────────────────────────────────────────────────────────────
# Type Conversions
# ─────────────────────────────────────────────────────────────────────────────

__neap_int_to_float() { echo "$1.0"; }
__neap_float_to_int() { echo "${1%.*}"; }
__neap_int_to_string() { jq -n --arg s "$1" '$s'; }
__neap_float_to_string() { jq -n --arg s "$1" '$s'; }
__neap_char_to_string() { jq -n --arg s "$1" '$s'; }
__neap_bool_to_string() { jq -n --arg s "$1" '$s'; }
__neap_string_identity() { echo "$1"; }

__neap_char_to_int() {
    printf '%d' "'$1"
}

__neap_int_to_char() {
    printf "\\$(printf '%03o' "$1")"
}

# ─────────────────────────────────────────────────────────────────────────────
# ADT Construction
# ─────────────────────────────────────────────────────────────────────────────

# Construct an ADT value with no payload
__neap_construct0() {
    local tag=$1
    jq -nc --arg t "$tag" '{_tag: $t}'
}

# Construct an ADT value with a payload
__neap_construct1() {
    local tag=$1 payload=$2
    jq -nc --arg t "$tag" --argjson p "$payload" '{_tag: $t, _0: $p}'
}

# ─────────────────────────────────────────────────────────────────────────────
# Tuple Operations
# ─────────────────────────────────────────────────────────────────────────────

# Project from tuple by index
__neap_tuple_proj() {
    local tuple=$1 idx=$2
    echo "$tuple" | jq -c ".[$idx]"
}

# ─────────────────────────────────────────────────────────────────────────────
# Record Operations
# ─────────────────────────────────────────────────────────────────────────────

# Get field from record
__neap_field() {
    local record=$1 field=$2
    echo "$record" | jq -c ".$field"
}

# ─────────────────────────────────────────────────────────────────────────────
# Command Execution (returns Result)
# ─────────────────────────────────────────────────────────────────────────────

# Run a shell command and return Result<string, {code: int, stderr: string}>
__neap_run_cmd() {
    local cmd=$1
    local stdout stderr exit_code

    # Create temp file for stderr
    local stderr_file=$(mktemp)
    trap "rm -f $stderr_file" EXIT

    # Run command, capturing stdout and stderr separately
    stdout=$(eval "$cmd" 2>"$stderr_file")
    exit_code=$?
    stderr=$(cat "$stderr_file")
    rm -f "$stderr_file"

    if [[ $exit_code -eq 0 ]]; then
        jq -nc --arg s "$stdout" '{"_tag":"Ok","_0":$s}'
    else
        jq -nc --arg e "$stderr" --argjson c "$exit_code" \
            '{"_tag":"Err","_0":{"code":$c,"stderr":$e}}'
    fi
}

# ─────────────────────────────────────────────────────────────────────────────
# I/O Operations
# ─────────────────────────────────────────────────────────────────────────────

# Print a value (unwraps JSON strings)
__neap_print() {
    local val=$1
    local val_type
    val_type=$(echo "$val" | jq -r 'type' 2>/dev/null)

    if [[ "$val_type" == "string" ]]; then
        echo "$val" | jq -r '.'
    else
        echo "$val"
    fi
}

# Print without newline
__neap_print_no_newline() {
    local val=$1
    local val_type
    val_type=$(echo "$val" | jq -r 'type' 2>/dev/null)

    if [[ "$val_type" == "string" ]]; then
        echo -n "$val" | jq -r '.'
    else
        echo -n "$val"
    fi
}

# Read a line from stdin
__neap_read_line() {
    local line
    read -r line
    jq -n --arg s "$line" '$s'
}

# Read entire file
__neap_read_file() {
    local path
    path=$(echo "$1" | jq -r '.')
    jq -Rs '.' < "$path"
}

# Write to file
__neap_write_file() {
    local path content
    path=$(echo "$1" | jq -r '.')
    content=$(echo "$2" | jq -r '.')
    echo "$content" > "$path"
}

# Get environment variable (returns Option<string>)
__neap_get_env() {
    local name
    name=$(echo "$1" | jq -r '.')
    local value="${(P)name}"

    if [[ -n "$value" ]]; then
        jq -nc --arg v "$value" '{"_tag":"Some","_0":$v}'
    else
        echo '{"_tag":"None"}'
    fi
}

# ─────────────────────────────────────────────────────────────────────────────
# Assertions and Errors
# ─────────────────────────────────────────────────────────────────────────────

__neap_assert() {
    if [[ $1 != "true" ]]; then
        echo "Assertion failed" >&2
        exit 1
    fi
}

__neap_panic() {
    local msg
    msg=$(echo "$1" | jq -r '.')
    echo "panic: $msg" >&2
    exit 1
}

# ══════════════════════════════════════════════════════════════════════════════
# End of Neap Runtime
# ══════════════════════════════════════════════════════════════════════════════
"#;
