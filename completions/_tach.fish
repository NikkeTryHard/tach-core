# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_tach_global_optspecs
	string join \n n/workers= k/keyword= m/markers= x/exitfirst maxfail= w/watch v/verbose q/quiet format= tb= coverage cov= junit-xml= no-isolation force-toxic durations= timeout= diagnose dry-run collect-only h/help V/version
end

function __fish_tach_needs_command
	# Figure out if the current invocation already has a command.
	set -l cmd (commandline -opc)
	set -e cmd[1]
	argparse -s (__fish_tach_global_optspecs) -- $cmd 2>/dev/null
	or return
	if set -q argv[1]
		# Also print the command, so this can be used to figure out what it is.
		echo $argv[1]
		return 1
	end
	return 0
end

function __fish_tach_using_subcommand
	set -l cmd (__fish_tach_needs_command)
	test -z "$cmd"
	and return 1
	contains -- $cmd[1] $argv
end

complete -c tach -n "__fish_tach_needs_command" -s n -l workers -d 'Number of workers for parallel test execution' -r
complete -c tach -n "__fish_tach_needs_command" -s k -l keyword -d 'Run tests matching the given substring expression' -r
complete -c tach -n "__fish_tach_needs_command" -s m -l markers -d 'Run tests matching the given marker expression' -r
complete -c tach -n "__fish_tach_needs_command" -l maxfail -d 'Exit after N failures (--maxfail=N)' -r
complete -c tach -n "__fish_tach_needs_command" -l format -d 'Output format (also: TACH_FORMAT env var)' -r -f -a "human\t'Human-readable CLI output (to stderr)'
json\t'Machine-readable NDJSON (to stdout)'"
complete -c tach -n "__fish_tach_needs_command" -l tb -d 'Traceback formatting style for failures' -r -f -a "short\t'First and last frames only'
long\t'Full traceback with locals (default)'
line\t'Single line per failure (file:line: message)'
native\t'Python\'s default traceback format (unmodified)'
no\t'No traceback output'"
complete -c tach -n "__fish_tach_needs_command" -l cov -d 'Source directories for coverage (can specify multiple)' -r
complete -c tach -n "__fish_tach_needs_command" -l junit-xml -d 'Path to generate JUnit XML report (also: TACH_JUNIT_XML env var)' -r -F
complete -c tach -n "__fish_tach_needs_command" -l durations -d 'Show timing for slowest N tests' -r
complete -c tach -n "__fish_tach_needs_command" -l timeout -d 'Global timeout in seconds for each test (default: 60)' -r
complete -c tach -n "__fish_tach_needs_command" -s x -l exitfirst -d 'Exit on first failure (fail fast)'
complete -c tach -n "__fish_tach_needs_command" -s w -l watch -d 'Watch for changes and re-run tests automatically'
complete -c tach -n "__fish_tach_needs_command" -s v -l verbose -d 'Increase verbosity (-v for verbose, -vv for very verbose)'
complete -c tach -n "__fish_tach_needs_command" -s q -l quiet -d 'Decrease verbosity (quiet mode)'
complete -c tach -n "__fish_tach_needs_command" -l coverage -d 'Enable coverage collection (PEP 669 sys.monitoring)'
complete -c tach -n "__fish_tach_needs_command" -l no-isolation -d 'Disable filesystem and network isolation'
complete -c tach -n "__fish_tach_needs_command" -l force-toxic -d 'Force toxic mode for all tests (no snapshot reuse)'
complete -c tach -n "__fish_tach_needs_command" -l diagnose -d 'Run system diagnostics and exit'
complete -c tach -n "__fish_tach_needs_command" -l dry-run -d 'Discover tests and show what would run without executing'
complete -c tach -n "__fish_tach_needs_command" -l collect-only -d 'Collect and list tests without running (alias for \'list\' command)'
complete -c tach -n "__fish_tach_needs_command" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c tach -n "__fish_tach_needs_command" -s V -l version -d 'Print version'
complete -c tach -n "__fish_tach_needs_command" -a "test" -d 'Run tests (default if no subcommand)'
complete -c tach -n "__fish_tach_needs_command" -a "list" -d 'List discovered tests without running'
complete -c tach -n "__fish_tach_needs_command" -a "self-test" -d 'Run self-diagnostics to verify kernel support'
complete -c tach -n "__fish_tach_needs_command" -a "version" -d 'Show version and build information'
complete -c tach -n "__fish_tach_needs_command" -a "completions" -d 'Generate shell completion scripts'
complete -c tach -n "__fish_tach_needs_command" -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c tach -n "__fish_tach_using_subcommand test" -s h -l help -d 'Print help'
complete -c tach -n "__fish_tach_using_subcommand list" -s h -l help -d 'Print help'
complete -c tach -n "__fish_tach_using_subcommand self-test" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c tach -n "__fish_tach_using_subcommand version" -s h -l help -d 'Print help'
complete -c tach -n "__fish_tach_using_subcommand completions" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c tach -n "__fish_tach_using_subcommand help; and not __fish_seen_subcommand_from test list self-test version completions help" -f -a "test" -d 'Run tests (default if no subcommand)'
complete -c tach -n "__fish_tach_using_subcommand help; and not __fish_seen_subcommand_from test list self-test version completions help" -f -a "list" -d 'List discovered tests without running'
complete -c tach -n "__fish_tach_using_subcommand help; and not __fish_seen_subcommand_from test list self-test version completions help" -f -a "self-test" -d 'Run self-diagnostics to verify kernel support'
complete -c tach -n "__fish_tach_using_subcommand help; and not __fish_seen_subcommand_from test list self-test version completions help" -f -a "version" -d 'Show version and build information'
complete -c tach -n "__fish_tach_using_subcommand help; and not __fish_seen_subcommand_from test list self-test version completions help" -f -a "completions" -d 'Generate shell completion scripts'
complete -c tach -n "__fish_tach_using_subcommand help; and not __fish_seen_subcommand_from test list self-test version completions help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
