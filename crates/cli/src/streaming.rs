//! Terminal adaptation of the application session boundary.
use std::{
    future::Future,
    io::{self, Read, Write},
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use probe_application::{CloseOrigin, SessionData, SessionError, SessionEvent, WebSocketExecution};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::{CliError, JSON_SCHEMA_VERSION, request::RunOptions};

type InputFactory<'a> =
    dyn FnMut(&mut dyn Read) -> io::Result<mpsc::Receiver<io::Result<String>>> + 'a;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutputMode {
    Human,
    Json,
    Quiet,
}
impl OutputMode {
    pub(crate) fn from_flags(json: bool, quiet: bool) -> Self {
        if quiet {
            Self::Quiet
        } else if json {
            Self::Json
        } else {
            Self::Human
        }
    }
}

pub(crate) struct LiveOutput<'a> {
    pub(crate) stdout: &'a mut dyn Write,
    pub(crate) stderr: &'a mut dyn Write,
    input: &'a mut InputFactory<'a>,
    pub(crate) mode: OutputMode,
    pub(crate) started: bool,
    pub(crate) exit_code: u8,
}
impl<'a> LiveOutput<'a> {
    pub(crate) fn new(
        stdout: &'a mut dyn Write,
        stderr: &'a mut dyn Write,
        input: &'a mut InputFactory<'a>,
    ) -> Self {
        Self {
            stdout,
            stderr,
            input,
            mode: OutputMode::Human,
            started: false,
            exit_code: 0,
        }
    }

    fn json_line(&mut self, value: Value) -> Result<(), CliError> {
        serde_json::to_writer(&mut self.stdout, &value)
            .map_err(|error| CliError::output(io::Error::other(error)))?;
        writeln!(self.stdout).map_err(CliError::output)?;
        self.stdout.flush().map_err(CliError::output)
    }

    fn failure(&mut self, error: &CliError, event: bool) -> Result<(), CliError> {
        // A terminal session failure wins over subsequent CLI shutdown failures.
        if self.exit_code != 0 {
            return Ok(());
        }
        self.exit_code = error.exit_code;
        if self.mode == OutputMode::Json {
            let mut value = json!({"schemaVersion": JSON_SCHEMA_VERSION, "error": {
                "category": error.category, "exitCode": error.exit_code, "message": error.message,
            }});
            if event {
                value["event"] = json!("error");
            }
            if let Some(details) = &error.details {
                value["error"]["details"] = details.clone();
            }
            self.json_line(value)
        } else {
            writeln!(self.stderr, "error[{}]: {}", error.category, error.message)
                .map_err(CliError::output)?;
            self.stderr.flush().map_err(CliError::output)
        }
    }

    fn event(&mut self, event: &SessionEvent) -> Result<(), CliError> {
        if let SessionEvent::Error(error) = event {
            return self.failure(&CliError::session(*error), true);
        }
        match self.mode {
            OutputMode::Quiet => Ok(()),
            OutputMode::Json => self.json_line(event_json(event)),
            OutputMode::Human => {
                match event {
                    SessionEvent::Opened { url } => writeln!(self.stdout, "Connected: {url}"),
                    SessionEvent::Sent(data) => writeln!(self.stdout, "> {}", data_human(data)),
                    SessionEvent::Received(data) => writeln!(self.stdout, "< {}", data_human(data)),
                    SessionEvent::Closed {
                        origin,
                        code,
                        reason,
                    } => {
                        write!(self.stdout, "Closed: {}", origin_name(*origin))
                            .map_err(CliError::output)?;
                        if let Some(code) = code {
                            write!(self.stdout, " code={code}").map_err(CliError::output)?;
                        }
                        if !reason.is_empty() {
                            write!(self.stdout, " reason={reason}").map_err(CliError::output)?;
                        }
                        writeln!(self.stdout)
                    }
                    SessionEvent::Error(_) => unreachable!(),
                }
                .map_err(CliError::output)?;
                self.stdout.flush().map_err(CliError::output)
            }
        }
    }
}

fn origin_name(origin: CloseOrigin) -> &'static str {
    match origin {
        CloseOrigin::Local => "local",
        CloseOrigin::Remote => "remote",
        CloseOrigin::Error => "error",
    }
}
fn data_human(data: &SessionData) -> String {
    match data {
        SessionData::Text(value) => value.clone(),
        SessionData::Binary(bytes) => format!("[binary/base64] {}", STANDARD.encode(bytes)),
    }
}
fn data_json(data: &SessionData) -> Value {
    match data {
        SessionData::Text(value) => json!({"type": "text", "value": value}),
        SessionData::Binary(bytes) => {
            json!({"type": "binary", "encoding": "base64", "value": STANDARD.encode(bytes)})
        }
    }
}
fn event_json(event: &SessionEvent) -> Value {
    let mut value = match event {
        SessionEvent::Opened { url } => json!({"event": "opened", "url": url}),
        SessionEvent::Sent(data) => json!({"event": "sent", "data": data_json(data)}),
        SessionEvent::Received(data) => json!({"event": "received", "data": data_json(data)}),
        SessionEvent::Closed {
            origin,
            code,
            reason,
        } => {
            json!({"event": "closed", "origin": origin_name(*origin), "code": code, "reason": reason})
        }
        SessionEvent::Error(_) => unreachable!("errors use the CLI category adapter"),
    };
    value["schemaVersion"] = json!(JSON_SCHEMA_VERSION);
    value
}

pub(crate) fn strip_terminator(mut line: String) -> String {
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    line
}

fn input_error() -> CliError {
    CliError {
        category: "stdin_error",
        message: "cannot read WebSocket input".into(),
        exit_code: crate::EXECUTION_EXIT_CODE,
        details: None,
    }
}

// Output is already unavailable: preserve that error while making cleanup
// interruptible. A consumed CLI deadline must not be polled a second time.
async fn finish_output_error(
    error: CliError,
    closed: impl Future<Output = ()>,
    interrupt: impl Future<Output = io::Result<()>>,
    timeout: impl Future<Output = ()>,
    timed_out: bool,
) -> Result<(), CliError> {
    if !timed_out {
        tokio::select! {
            _ = closed => {},
            _ = interrupt => {},
            _ = timeout => {},
        }
    }
    Err(error)
}

enum SessionEnd {
    Drained,
    Cancelled,
}

pub(crate) fn run(
    execution: WebSocketExecution,
    options: &RunOptions<'_>,
    stdin: &mut impl Read,
    output: &mut LiveOutput<'_>,
) -> Result<(), CliError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| CliError {
            category: "network_execution",
            message: "cannot start asynchronous streaming runtime".into(),
            exit_code: crate::EXECUTION_EXIT_CODE,
            details: None,
        })?;
    runtime.block_on(async {
        let deadline = options
            .timeout
            .map(|seconds| {
                tokio::time::Instant::now()
                    .checked_add(Duration::from_secs(seconds))
                    .ok_or_else(|| {
                        CliError::invalid_arguments("--timeout exceeds the supported duration")
                    })
            })
            .transpose()?;
        let timeout = async {
            if let Some(deadline) = deadline {
                tokio::time::sleep_until(deadline).await;
            } else {
                std::future::pending::<()>().await;
            }
        };
        tokio::pin!(timeout);
        let interrupt = tokio::signal::ctrl_c();
        tokio::pin!(interrupt);
        let mut session = tokio::select! {
            result = execution.connect() => result.map_err(CliError::session)?,
            _ = &mut timeout => return Err(CliError::session(SessionError::Timeout)),
            _ = &mut interrupt => return Err(CliError::cancelled()),
        };
        let mut input = match (output.input)(stdin) {
            Ok(input) => input,
            Err(_) => {
                session.close();
                session.wait_closed().await;
                return Err(input_error());
            }
        };
        output.started = true;
        let sender = session.sender();
        let sends = options.sends.to_vec();
        // A separate task may wait for command capacity without preventing the
        // consumer from draining the bounded event queue. EOF only ends this task.
        let mut producer = tokio::spawn(async move {
            for text in sends {
                match sender.send(SessionData::Text(text)).await {
                    Ok(()) => {}
                    Err(SessionError::Closed) => return Ok(()),
                    Err(error) => return Err(CliError::session(error)),
                }
            }
            while let Some(line) = input.recv().await {
                let text = line.map_err(|_| input_error())?;
                match sender.send(SessionData::Text(text)).await {
                    Ok(()) => {}
                    Err(SessionError::Closed) => return Ok(()),
                    Err(error) => return Err(CliError::session(error)),
                }
            }
            Ok::<(), CliError>(())
        });
        let mut accepting = true;
        let mut closing = false;
        let mut timed_out = false;
        let mut received = 0_u64;
        let mut input_error = None;
        let result = async {
            loop {
                tokio::select! {
                    event = session.next_event() => {
                        let Some(event) = event else { break; };
                        output.event(&event)?;
                        if matches!(event, SessionEvent::Received(_)) {
                            received += 1;
                            if options.max_messages == Some(received) && !closing {
                                closing = true;
                                session.close();
                                producer.abort();
                            }
                        }
                        if matches!(event, SessionEvent::Closed { .. }) { break; }
                    }
                    _ = &mut interrupt => {
                        interrupt.set(tokio::signal::ctrl_c());
                        producer.abort();
                        if closing {
                            return Ok(SessionEnd::Cancelled);
                        }
                        closing = true;
                        session.close();
                    }
                    _ = &mut timeout, if !timed_out => {
                        timed_out = true;
                        closing = true;
                        session.close();
                        producer.abort();
                    }
                    result = &mut producer, if accepting => {
                        accepting = false;
                        match result {
                            Ok(Err(error)) => {
                                input_error = Some(error);
                                closing = true;
                                session.close();
                            }
                            Err(error) if !error.is_cancelled() => {
                                input_error = Some(CliError::session(SessionError::Connection));
                                closing = true;
                                session.close();
                            }
                            _ => {},
                        }
                    }
                }
            }
            session.wait_closed().await;
            if timed_out {
                output.failure(&CliError::session(SessionError::Timeout), false)?;
            } else if let Some(error) = input_error {
                output.failure(&error, false)?;
            }
            Ok(SessionEnd::Drained)
        }
        .await;
        producer.abort();
        match result {
            Ok(SessionEnd::Cancelled) => {
                // Dropping the owner and runtime cancels the bounded close wait.
                drop(session);
                output.failure(&CliError::cancelled(), false)
            }
            Ok(SessionEnd::Drained) => Ok(()),
            Err(error) => {
                session.close();
                finish_output_error(
                    error,
                    session.wait_closed(),
                    &mut interrupt,
                    &mut timeout,
                    timed_out,
                )
                .await
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Flushed {
        bytes: Vec<u8>,
        flushes: usize,
    }
    impl Write for Flushed {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            Ok(())
        }
    }

    #[tokio::test(start_paused = true)]
    async fn output_error_cleanup_is_interruptible_and_preserves_the_original_error() {
        for finish in ["closed", "interrupt", "timeout", "already_timed_out"] {
            let started = tokio::time::Instant::now();
            let error = finish_output_error(
                CliError::output(io::Error::other("output unavailable")),
                async {
                    if finish != "closed" {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                },
                async {
                    if finish != "interrupt" {
                        std::future::pending::<()>().await;
                    }
                    Ok(())
                },
                async {
                    assert_ne!(
                        finish, "already_timed_out",
                        "completed deadline was polled again"
                    );
                    if finish == "timeout" {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    } else {
                        std::future::pending::<()>().await;
                    }
                },
                finish == "already_timed_out",
            )
            .await
            .unwrap_err();
            assert_eq!(error.category, "output_error");
            assert_eq!(error.message, "output unavailable");
            assert_eq!(error.exit_code, crate::EXECUTION_EXIT_CODE);
            let expected = if finish == "timeout" {
                Duration::from_secs(1)
            } else {
                Duration::ZERO
            };
            assert_eq!(tokio::time::Instant::now() - started, expected, "{finish}");
        }
    }

    #[test]
    fn terminal_session_error_wins_over_timeout_before_closed() {
        for mode in [OutputMode::Human, OutputMode::Json, OutputMode::Quiet] {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let mut input = |_: &mut dyn Read| unreachable!();
            let exit_code = {
                let mut output = LiveOutput::new(&mut stdout, &mut stderr, &mut input);
                output.mode = mode;
                output
                    .event(&SessionEvent::Error(SessionError::Protocol))
                    .unwrap();
                // The CLI deadline can become ready between Error and Closed.
                output
                    .failure(&CliError::session(SessionError::Timeout), false)
                    .unwrap();
                output
                    .event(&SessionEvent::Closed {
                        origin: CloseOrigin::Error,
                        code: None,
                        reason: String::new(),
                    })
                    .unwrap();
                output.exit_code
            };
            assert_eq!(exit_code, crate::EXECUTION_EXIT_CODE);
            if mode == OutputMode::Json {
                let records: Vec<Value> = String::from_utf8(stdout)
                    .unwrap()
                    .lines()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect();
                assert_eq!(records.len(), 2);
                assert_eq!(records[0]["event"], "error");
                assert_eq!(records[0]["error"]["category"], "network_execution");
                assert_eq!(records[1]["event"], "closed");
                assert!(stderr.is_empty());
            } else {
                let diagnostics = String::from_utf8(stderr).unwrap();
                assert_eq!(diagnostics.lines().count(), 1);
                assert!(diagnostics.starts_with("error[network_execution]:"));
                assert!(!diagnostics.contains("request_timeout"));
            }
        }
    }

    #[test]
    fn binary_outbound_and_close_metadata_flush_in_each_output_mode() {
        let events = [
            SessionEvent::Sent(SessionData::Binary(vec![0, 1, 2])),
            SessionEvent::Closed {
                origin: CloseOrigin::Local,
                code: Some(1000),
                reason: "bye".into(),
            },
        ];
        for mode in [OutputMode::Human, OutputMode::Json, OutputMode::Quiet] {
            let mut writer = Flushed::default();
            let mut stderr = Vec::new();
            let mut input = |_: &mut dyn Read| unreachable!();
            {
                let mut output = LiveOutput::new(&mut writer, &mut stderr, &mut input);
                output.mode = mode;
                for event in &events {
                    output.event(event).unwrap();
                }
            }
            assert!(stderr.is_empty());
            let text = String::from_utf8(writer.bytes).unwrap();
            if mode == OutputMode::Quiet {
                assert!(text.is_empty());
                assert_eq!(writer.flushes, 0);
            } else {
                assert_eq!(writer.flushes, events.len());
                if mode == OutputMode::Human {
                    assert_eq!(
                        text,
                        "> [binary/base64] AAEC\nClosed: local code=1000 reason=bye\n"
                    );
                } else {
                    let records: Vec<Value> = text
                        .lines()
                        .map(|line| serde_json::from_str(line).unwrap())
                        .collect();
                    assert_eq!(
                        records[0],
                        json!({"schemaVersion":1,"event":"sent","data":{"type":"binary","encoding":"base64","value":"AAEC"}})
                    );
                    assert_eq!(
                        records[1],
                        json!({"schemaVersion":1,"event":"closed","origin":"local","code":1000,"reason":"bye"})
                    );
                }
            }
        }
    }
}
