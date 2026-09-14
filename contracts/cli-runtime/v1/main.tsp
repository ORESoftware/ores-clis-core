namespace OresCliRuntime.V1;

/** Primary stdout selection before terminal resolution. */
enum OutputMode {
  auto,
  human,
  json,
}

/** Primary stdout selection after terminal resolution. */
enum ResolvedOutputMode {
  human,
  json,
}

/** Human-readable color preference. JSON output is always ANSI-free. */
enum ColorMode {
  auto,
  always,
  never,
}

/** Shared log threshold in increasing verbosity order. */
enum LogLevel {
  silent,
  quiet,
  error,
  warn,
  info,
  debug,
  trace,
}

/** Terminal capabilities captured once at process startup. */
model TerminalState {
  stdin_tty: boolean;
  stdout_tty: boolean;
  stderr_tty: boolean;
}

/** Conventional environment hints applied only to automatic color policy. */
model EnvironmentHints {
  no_color: boolean;
  force_color: boolean;
}

/** Parser-agnostic CLI preferences shared by all Rust consumers. */
model CliPolicy {
  output_mode: OutputMode;
  color_mode: ColorMode;
  log_level: LogLevel;
}

/** Resolved runtime state after terminal/environment policy is applied. */
model ResolvedCliPolicy {
  output_mode: ResolvedOutputMode;
  color_stdout: boolean;
  color_stderr: boolean;
  log_level: LogLevel;
  flush_each_record: boolean;
  terminal_state: TerminalState;
  environment_hints: EnvironmentHints;
}
