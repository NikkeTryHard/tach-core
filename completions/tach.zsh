#compdef tach

autoload -U is-at-least

_tach() {
    typeset -A opt_args
    typeset -a _arguments_options
    local ret=1

    if is-at-least 5.2; then
        _arguments_options=(-s -S -C)
    else
        _arguments_options=(-s -C)
    fi

    local context curcontext="$curcontext" state line
    _arguments "${_arguments_options[@]}" : \
'-n+[Number of workers for parallel test execution]:WORKERS:_default' \
'--workers=[Number of workers for parallel test execution]:WORKERS:_default' \
'-k+[Run tests matching the given substring expression]:EXPRESSION:_default' \
'--keyword=[Run tests matching the given substring expression]:EXPRESSION:_default' \
'-m+[Run tests matching the given marker expression]:MARKERS:_default' \
'--markers=[Run tests matching the given marker expression]:MARKERS:_default' \
'--maxfail=[Exit after N failures (--maxfail=N)]:N:_default' \
'--format=[Output format (also\: TACH_FORMAT env var)]:FORMAT:((human\:"Human-readable CLI output (to stderr)"
json\:"Machine-readable NDJSON (to stdout)"))' \
'--tb=[Traceback formatting style for failures]:TRACEBACK:((short\:"First and last frames only"
long\:"Full traceback with locals (default)"
line\:"Single line per failure (file\:line\: message)"
native\:"Python'\''s default traceback format (unmodified)"
no\:"No traceback output"))' \
'*--cov=[Source directories for coverage (can specify multiple)]:PATH:_default' \
'--junit-xml=[Path to generate JUnit XML report (also\: TACH_JUNIT_XML env var)]:PATH:_files' \
'--durations=[Show timing for slowest N tests]:N:_default' \
'--timeout=[Global timeout in seconds for each test (default\: 60)]:SECONDS:_default' \
'-x[Exit on first failure (fail fast)]' \
'--exitfirst[Exit on first failure (fail fast)]' \
'-w[Watch for changes and re-run tests automatically]' \
'--watch[Watch for changes and re-run tests automatically]' \
'*-v[Increase verbosity (-v for verbose, -vv for very verbose)]' \
'*--verbose[Increase verbosity (-v for verbose, -vv for very verbose)]' \
'-q[Decrease verbosity (quiet mode)]' \
'--quiet[Decrease verbosity (quiet mode)]' \
'--coverage[Enable coverage collection (PEP 669 sys.monitoring)]' \
'--no-isolation[Disable filesystem and network isolation]' \
'--force-toxic[Force toxic mode for all tests (no snapshot reuse)]' \
'--diagnose[Run system diagnostics and exit]' \
'--dry-run[Discover tests and show what would run without executing]' \
'--collect-only[Collect and list tests without running (alias for '\''list'\'' command)]' \
'-h[Print help (see more with '\''--help'\'')]' \
'--help[Print help (see more with '\''--help'\'')]' \
'-V[Print version]' \
'--version[Print version]' \
'::path -- Test directory or file pattern:_default' \
'::pytest_args -- Extra arguments to pass to pytest shim:_default' \
":: :_tach_commands" \
"*::: :->tach" \
&& ret=0
    case $state in
    (tach)
        words=($line[3] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:tach-command-$line[3]:"
        case $line[3] in
            (test)
_arguments "${_arguments_options[@]}" : \
'-h[Print help]' \
'--help[Print help]' \
&& ret=0
;;
(list)
_arguments "${_arguments_options[@]}" : \
'-h[Print help]' \
'--help[Print help]' \
&& ret=0
;;
(self-test)
_arguments "${_arguments_options[@]}" : \
'-h[Print help (see more with '\''--help'\'')]' \
'--help[Print help (see more with '\''--help'\'')]' \
&& ret=0
;;
(version)
_arguments "${_arguments_options[@]}" : \
'-h[Print help]' \
'--help[Print help]' \
&& ret=0
;;
(completions)
_arguments "${_arguments_options[@]}" : \
'-h[Print help (see more with '\''--help'\'')]' \
'--help[Print help (see more with '\''--help'\'')]' \
':shell -- Shell to generate completions for:((bash\:"Bash shell"
zsh\:"Zsh shell"
fish\:"Fish shell"
power-shell\:"PowerShell"
elvish\:"Elvish shell"))' \
&& ret=0
;;
(help)
_arguments "${_arguments_options[@]}" : \
":: :_tach__help_commands" \
"*::: :->help" \
&& ret=0

    case $state in
    (help)
        words=($line[1] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:tach-help-command-$line[1]:"
        case $line[1] in
            (test)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(list)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(self-test)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(version)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(completions)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
(help)
_arguments "${_arguments_options[@]}" : \
&& ret=0
;;
        esac
    ;;
esac
;;
        esac
    ;;
esac
}

(( $+functions[_tach_commands] )) ||
_tach_commands() {
    local commands; commands=(
'test:Run tests (default if no subcommand)' \
'list:List discovered tests without running' \
'self-test:Run self-diagnostics to verify kernel support' \
'version:Show version and build information' \
'completions:Generate shell completion scripts' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'tach commands' commands "$@"
}
(( $+functions[_tach__completions_commands] )) ||
_tach__completions_commands() {
    local commands; commands=()
    _describe -t commands 'tach completions commands' commands "$@"
}
(( $+functions[_tach__help_commands] )) ||
_tach__help_commands() {
    local commands; commands=(
'test:Run tests (default if no subcommand)' \
'list:List discovered tests without running' \
'self-test:Run self-diagnostics to verify kernel support' \
'version:Show version and build information' \
'completions:Generate shell completion scripts' \
'help:Print this message or the help of the given subcommand(s)' \
    )
    _describe -t commands 'tach help commands' commands "$@"
}
(( $+functions[_tach__help__completions_commands] )) ||
_tach__help__completions_commands() {
    local commands; commands=()
    _describe -t commands 'tach help completions commands' commands "$@"
}
(( $+functions[_tach__help__help_commands] )) ||
_tach__help__help_commands() {
    local commands; commands=()
    _describe -t commands 'tach help help commands' commands "$@"
}
(( $+functions[_tach__help__list_commands] )) ||
_tach__help__list_commands() {
    local commands; commands=()
    _describe -t commands 'tach help list commands' commands "$@"
}
(( $+functions[_tach__help__self-test_commands] )) ||
_tach__help__self-test_commands() {
    local commands; commands=()
    _describe -t commands 'tach help self-test commands' commands "$@"
}
(( $+functions[_tach__help__test_commands] )) ||
_tach__help__test_commands() {
    local commands; commands=()
    _describe -t commands 'tach help test commands' commands "$@"
}
(( $+functions[_tach__help__version_commands] )) ||
_tach__help__version_commands() {
    local commands; commands=()
    _describe -t commands 'tach help version commands' commands "$@"
}
(( $+functions[_tach__list_commands] )) ||
_tach__list_commands() {
    local commands; commands=()
    _describe -t commands 'tach list commands' commands "$@"
}
(( $+functions[_tach__self-test_commands] )) ||
_tach__self-test_commands() {
    local commands; commands=()
    _describe -t commands 'tach self-test commands' commands "$@"
}
(( $+functions[_tach__test_commands] )) ||
_tach__test_commands() {
    local commands; commands=()
    _describe -t commands 'tach test commands' commands "$@"
}
(( $+functions[_tach__version_commands] )) ||
_tach__version_commands() {
    local commands; commands=()
    _describe -t commands 'tach version commands' commands "$@"
}

if [ "$funcstack[1]" = "_tach" ]; then
    _tach "$@"
else
    compdef _tach tach
fi
