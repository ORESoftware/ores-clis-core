use std::io::{self, Write};

/// Conventional ownership of a CLI output stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamRole {
    /// Primary command results and machine-readable protocol data on stdout.
    Primary,
    /// Diagnostics, logs, progress, and operator-facing status on stderr.
    Diagnostics,
}

/// Flush strategy for a logical line emitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlushPolicy {
    /// Flush after every record; preferred for terminals, pipes, and long-lived streams.
    #[default]
    EveryRecord,
    /// Buffer records until the caller explicitly flushes; useful for bounded file output.
    OnDemand,
}

/// Top-level interpretation of an emission attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmitDisposition {
    /// The operation completed normally.
    Written,
    /// A downstream consumer closed its pipe early (for example `head`).
    ConsumerClosed,
}

/// Treat `BrokenPipe` as normal early-consumer termination while preserving all
/// unrelated I/O failures.
pub fn top_level_io(result: io::Result<()>) -> io::Result<EmitDisposition> {
    match result {
        Ok(()) => Ok(EmitDisposition::Written),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {
            Ok(EmitDisposition::ConsumerClosed)
        }
        Err(error) => Err(error),
    }
}

/// Explicit stream-role emitter for CLI protocols.
///
/// This type deliberately does not choose stdout/stderr itself. The consumer
/// passes the desired writer, while `StreamRole` prevents accidental machine
/// records on a diagnostics channel and accidental progress records on a
/// primary machine-data channel.
#[derive(Debug)]
pub struct ProtocolEmitter<W> {
    writer: W,
    role: StreamRole,
    flush_policy: FlushPolicy,
}

impl<W: Write> ProtocolEmitter<W> {
    /// Create an emitter with per-record flushing.
    #[must_use]
    pub const fn new(writer: W, role: StreamRole) -> Self {
        Self {
            writer,
            role,
            flush_policy: FlushPolicy::EveryRecord,
        }
    }

    /// Override the flushing policy.
    #[must_use]
    pub const fn with_flush_policy(mut self, flush_policy: FlushPolicy) -> Self {
        self.flush_policy = flush_policy;
        self
    }

    /// Return the stream role owned by this emitter.
    #[must_use]
    pub const fn role(&self) -> StreamRole {
        self.role
    }

    /// Emit a human-readable primary result line.
    pub fn emit_primary_human_line(&mut self, value: &str) -> io::Result<()> {
        self.require_role(StreamRole::Primary)?;
        self.write_line(value)
    }

    /// Emit one machine-readable JSON/NDJSON record on the primary stream.
    ///
    /// Literal ANSI escapes and literal CR/LF characters are rejected. JSON
    /// strings containing escaped `\\n` remain valid because they do not contain
    /// a literal newline byte.
    pub fn emit_primary_machine_record(&mut self, value: &str) -> io::Result<()> {
        self.require_role(StreamRole::Primary)?;
        validate_machine_record(value)?;
        self.write_line(value)
    }

    /// Emit one diagnostic/progress line on the diagnostics stream.
    pub fn emit_diagnostic_line(&mut self, value: &str) -> io::Result<()> {
        self.require_role(StreamRole::Diagnostics)?;
        self.write_line(value)
    }

    /// Explicitly flush the underlying writer.
    pub fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }

    /// Recover the wrapped writer.
    #[must_use]
    pub fn into_inner(self) -> W {
        self.writer
    }

    fn require_role(&self, required: StreamRole) -> io::Result<()> {
        if self.role == required {
            return Ok(());
        }

        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            match required {
                StreamRole::Primary => {
                    "primary command results must use the primary/stdout stream"
                }
                StreamRole::Diagnostics => {
                    "diagnostics and progress must use the diagnostics/stderr stream"
                }
            },
        ))
    }

    fn write_line(&mut self, value: &str) -> io::Result<()> {
        self.writer.write_all(value.as_bytes())?;
        self.writer.write_all(b"\n")?;
        if self.flush_policy == FlushPolicy::EveryRecord {
            self.writer.flush()?;
        }
        Ok(())
    }
}

fn validate_machine_record(value: &str) -> io::Result<()> {
    if value.contains('\u{1b}') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "machine output must not contain ANSI escape sequences",
        ));
    }
    if value.contains('\n') || value.contains('\r') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "one machine record must not contain literal CR/LF framing bytes",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct PartialWriter {
        bytes: Vec<u8>,
        max_per_write: usize,
        flushes: usize,
    }

    impl PartialWriter {
        fn new(max_per_write: usize) -> Self {
            Self {
                bytes: Vec::new(),
                max_per_write,
                flushes: 0,
            }
        }
    }

    impl Write for PartialWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            let amount = buffer.len().min(self.max_per_write.max(1));
            self.bytes.extend_from_slice(&buffer[..amount]);
            Ok(amount)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            Ok(())
        }
    }

    #[derive(Debug)]
    struct BrokenPipeWriter;

    impl Write for BrokenPipeWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "consumer closed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct FlushFailWriter {
        bytes: Vec<u8>,
    }

    impl Write for FlushFailWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::Other, "flush failed"))
        }
    }

    #[test]
    fn primary_machine_records_are_ansi_free_and_single_line() {
        let mut emitter = ProtocolEmitter::new(Vec::<u8>::new(), StreamRole::Primary);
        emitter.emit_primary_machine_record("{\"ok\":true}").unwrap();
        assert!(emitter
            .emit_primary_machine_record("\u{1b}[31m{\"ok\":false}")
            .is_err());
        assert!(emitter
            .emit_primary_machine_record("{\"bad\":\"literal\nnewline\"}")
            .is_err());
    }

    #[test]
    fn diagnostics_cannot_interleave_on_primary_machine_stream() {
        let mut primary = ProtocolEmitter::new(Vec::<u8>::new(), StreamRole::Primary);
        assert!(primary.emit_diagnostic_line("progress 1/2").is_err());

        let mut diagnostics =
            ProtocolEmitter::new(Vec::<u8>::new(), StreamRole::Diagnostics);
        assert!(diagnostics
            .emit_primary_machine_record("{\"ok\":true}")
            .is_err());
    }

    #[test]
    fn write_all_preserves_records_across_partial_writes() {
        let writer = PartialWriter::new(2);
        let mut emitter = ProtocolEmitter::new(writer, StreamRole::Primary);
        emitter.emit_primary_human_line("abcdef").unwrap();
        let writer = emitter.into_inner();
        assert_eq!(writer.bytes, b"abcdef\n");
        assert_eq!(writer.flushes, 1);
    }

    #[test]
    fn every_record_flushes_and_on_demand_does_not() {
        let writer = PartialWriter::new(32);
        let mut streaming = ProtocolEmitter::new(writer, StreamRole::Diagnostics);
        streaming.emit_diagnostic_line("one").unwrap();
        assert_eq!(streaming.into_inner().flushes, 1);

        let writer = PartialWriter::new(32);
        let mut buffered = ProtocolEmitter::new(writer, StreamRole::Primary)
            .with_flush_policy(FlushPolicy::OnDemand);
        buffered.emit_primary_human_line("one").unwrap();
        let mut writer = buffered.into_inner();
        assert_eq!(writer.flushes, 0);
        writer.flush().unwrap();
        assert_eq!(writer.flushes, 1);
    }

    #[test]
    fn head_style_broken_pipe_is_a_clean_top_level_disposition() {
        let mut emitter = ProtocolEmitter::new(BrokenPipeWriter, StreamRole::Primary);
        let result = top_level_io(emitter.emit_primary_human_line("record"));
        assert_eq!(result.unwrap(), EmitDisposition::ConsumerClosed);
    }

    #[test]
    fn closed_diagnostics_pipe_can_be_classified_without_touching_primary_data() {
        let mut diagnostics = ProtocolEmitter::new(BrokenPipeWriter, StreamRole::Diagnostics);
        let result = top_level_io(diagnostics.emit_diagnostic_line("progress"));
        assert_eq!(result.unwrap(), EmitDisposition::ConsumerClosed);
    }

    #[test]
    fn unrelated_io_failures_are_not_swallowed() {
        let error = io::Error::new(io::ErrorKind::PermissionDenied, "denied");
        let result = top_level_io(Err(error));
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn explicit_flush_failures_are_preserved() {
        let mut emitter = ProtocolEmitter::new(FlushFailWriter::default(), StreamRole::Primary);
        let error = emitter.emit_primary_human_line("record").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
    }
}
