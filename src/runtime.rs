use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

use anyhow::{Context, Result};
use serde_json::Value;

use crate::state::{ClaudeConfig, Runtime};

#[derive(Debug)]
pub struct RuntimeOutcome {
    pub status: ExitStatus,
    pub pid: Option<u32>,
}

/// Run a supported runtime with the provided prompt.
pub fn run_runtime(
    runtime: Runtime,
    prompt: &str,
    repo_root: &Path,
    worktree: &Path,
    background: bool,
    claude_config: Option<ClaudeConfig>,
) -> Result<RuntimeOutcome> {
    println!("=== LLMC PROMPT ===");
    println!("{prompt}");
    println!("=== END PROMPT ===\n");

    match runtime {
        Runtime::Codex => self::run_codex(prompt, repo_root, worktree, background),
        Runtime::Claude => self::run_claude(prompt, repo_root, worktree, background, claude_config),
        _ => Err(anyhow::anyhow!("Runtime {runtime:?} is not supported yet")),
    }
}

struct StreamState {
    current_tool: Option<String>,
    accumulated_input: String,
}

impl StreamState {
    fn new() -> Self {
        Self { current_tool: None, accumulated_input: String::new() }
    }
}

/// Process Claude's stream-json output and print readable text.
fn process_claude_stream(reader: impl BufRead) -> Result<()> {
    let mut state = StreamState::new();

    for line in reader.lines() {
        let line = line.context("Failed to read line from claude output")?;
        if line.trim().is_empty() {
            continue;
        }

        let parsed: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                // If not valid JSON, print as-is (might be error output)
                eprintln!("{line}");
                continue;
            }
        };

        // Extract event type
        let event_type = parsed.get("type").and_then(|v| v.as_str());

        match event_type {
            Some("stream_event") => {
                if let Some(event) = parsed.get("event") {
                    process_stream_event(event, &mut state)?;
                }
            }
            Some("tool_result") => {
                process_tool_result(&parsed, &mut state)?;
            }
            Some("error") => {
                if let Some(error_msg) = parsed.get("error").and_then(|e| e.get("message")) {
                    eprintln!("\n[ERROR] {}", error_msg.as_str().unwrap_or("Unknown error"));
                }
            }
            _ => {
                // Silently ignore unknown event types (message_start,
                // message_stop, assistant, etc.)
            }
        }
    }

    Ok(())
}

fn process_stream_event(event: &Value, state: &mut StreamState) -> Result<()> {
    let event_type = event.get("type").and_then(|v| v.as_str());

    match event_type {
        Some("content_block_start") => {
            if let Some(content_block) = event.get("content_block") {
                let block_type = content_block.get("type").and_then(|v| v.as_str());
                match block_type {
                    Some("thinking") => {
                        print!("\n[Thinking...] ");
                        std::io::Write::flush(&mut std::io::stdout())?;
                    }
                    Some("tool_use") => {
                        if let Some(name) = content_block.get("name").and_then(|v| v.as_str()) {
                            state.current_tool = Some(name.to_string());
                            state.accumulated_input.clear();

                            // Print initial tool header
                            print!("\n[Tool: {name}");
                            std::io::Write::flush(&mut std::io::stdout())?;
                        }
                    }
                    _ => {}
                }
            }
        }
        Some("content_block_delta") => {
            if let Some(delta) = event.get("delta") {
                let delta_type = delta.get("type").and_then(|v| v.as_str());
                match delta_type {
                    Some("text_delta") => {
                        if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                            print!("{text}");
                            std::io::Write::flush(&mut std::io::stdout())?;
                        }
                    }
                    Some("thinking_delta") => {
                        if let Some(text) = delta.get("thinking").and_then(|v| v.as_str()) {
                            print!("{text}");
                            std::io::Write::flush(&mut std::io::stdout())?;
                        }
                    }
                    Some("input_json_delta") => {
                        if let Some(partial) = delta.get("partial_json").and_then(|v| v.as_str()) {
                            state.accumulated_input.push_str(partial);
                        }
                    }
                    _ => {}
                }
            }
        }
        Some("content_block_stop") => {
            // Display tool details if we accumulated input
            if state.current_tool.is_some() && !state.accumulated_input.is_empty() {
                if let Ok(input) = serde_json::from_str::<Value>(&state.accumulated_input) {
                    display_tool_details(state.current_tool.as_deref().unwrap(), &input)?;
                }
                state.current_tool = None;
                state.accumulated_input.clear();
            } else if state.current_tool.is_some() {
                // Close the bracket if no input was shown
                println!("]");
                state.current_tool = None;
            } else {
                println!(); // Newline after text/thinking blocks
            }
        }
        _ => {}
    }

    Ok(())
}

fn display_tool_details(tool_name: &str, input: &Value) -> Result<()> {
    match tool_name {
        "Read" => {
            if let Some(path) = input.get("file_path").and_then(|v| v.as_str()) {
                println!(": {path}]");
            } else {
                println!("]");
            }
        }
        "Edit" | "Write" => {
            if let Some(path) = input.get("file_path").and_then(|v| v.as_str()) {
                println!(": {path}]");
            } else {
                println!("]");
            }
        }
        "Bash" => {
            if let Some(cmd) = input.get("command").and_then(|v| v.as_str()) {
                println!(": {cmd}]");
            } else {
                println!("]");
            }
        }
        "TodoWrite" => {
            println!("]");
            if let Some(todos) = input.get("todos").and_then(|v| v.as_array()) {
                for todo in todos {
                    if let Some(content) = todo.get("content").and_then(|v| v.as_str())
                        && let Some(status) = todo.get("status").and_then(|v| v.as_str())
                    {
                        let marker = match status {
                            "completed" => "✓",
                            "in_progress" => "→",
                            _ => "·",
                        };
                        println!("  {marker} {content}");
                    }
                }
            }
        }
        "Glob" => {
            if let Some(pattern) = input.get("pattern").and_then(|v| v.as_str()) {
                print!(": {pattern}");
                if let Some(path) = input.get("path").and_then(|v| v.as_str()) {
                    println!(" in {path}]");
                } else {
                    println!("]");
                }
            } else {
                println!("]");
            }
        }
        "Grep" => {
            if let Some(pattern) = input.get("pattern").and_then(|v| v.as_str()) {
                println!(": \"{pattern}\"]");
            } else {
                println!("]");
            }
        }
        _ => {
            // For other tools, just close the bracket
            println!("]");
        }
    }
    Ok(())
}

fn process_tool_result(parsed: &Value, _state: &mut StreamState) -> Result<()> {
    // Extract tool name and result - only process Bash results
    if let Some("Bash") = parsed.get("tool_name").and_then(|v| v.as_str()) {
        // Show exit code and output summary for bash commands
        if let Some(result) = parsed.get("result") {
            if let Some(exit_code) = result.get("exit_code").and_then(Value::as_i64) {
                if exit_code == 0 {
                    println!("  → Exit code: {exit_code} (success)");
                } else {
                    println!("  → Exit code: {exit_code} (FAILED)");
                }
            }

            // Show truncated output
            if let Some(output) = result.get("output").and_then(|v| v.as_str()) {
                let lines: Vec<_> = output.lines().collect();
                if lines.len() <= 5 {
                    for line in lines {
                        println!("  {line}");
                    }
                } else {
                    for line in &lines[..3] {
                        println!("  {line}");
                    }
                    println!("  ... ({} more lines)", lines.len() - 3);
                }
            }
        }
    }
    Ok(())
}

fn run_codex(
    prompt: &str,
    repo_root: &Path,
    worktree: &Path,
    background: bool,
) -> Result<RuntimeOutcome> {
    let mut command = Command::new("codex");
    command
        .arg("-a")
        .arg("never")
        .arg("exec")
        .arg("-C")
        .arg(worktree)
        .arg("--sandbox")
        .arg("workspace-write")
        .arg("--add-dir")
        .arg(repo_root.join(".git"))
        .arg(prompt)
        .env("WORKTREE", worktree)
        .current_dir(worktree);

    if background {
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    } else {
        command.stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit());
    }

    let mut child =
        command.spawn().with_context(|| format!("Failed to spawn codex in {worktree:?}"))?;
    let pid = Some(child.id());
    let status =
        child.wait().with_context(|| format!("Failed to wait for codex in {worktree:?}"))?;

    Ok(RuntimeOutcome { status, pid })
}

fn run_claude(
    prompt: &str,
    _repo_root: &Path,
    worktree: &Path,
    background: bool,
    claude_config: Option<ClaudeConfig>,
) -> Result<RuntimeOutcome> {
    let config = claude_config.unwrap_or_default();
    let mut command = Command::new("claude");

    if config.interactive {
        // Interactive mode: pass prompt as positional argument
        command.arg(prompt).env("WORKTREE", worktree).current_dir(worktree);
    } else {
        // Non-interactive mode: use -p with stream-json output
        command
            .arg("-p")
            .arg(prompt)
            .arg("--verbose")
            .arg("--output-format")
            .arg("stream-json")
            .arg("--include-partial-messages")
            .arg("--permission-mode")
            .arg("bypassPermissions")
            .env("WORKTREE", worktree)
            .current_dir(worktree);
    }

    if let Some(model) = &config.model {
        command.arg("--model").arg(model);
    }

    if config.no_thinking {
        command.arg("--no-thinking");
    }

    if let Some(sandbox) = &config.sandbox {
        command.arg("--sandbox").arg(sandbox);
    }

    if config.skip_permissions {
        command.arg("--dangerously-skip-permissions");
    }

    if let Some(allowed_tools) = &config.allowed_tools {
        command.arg("--allowedTools").arg(allowed_tools);
    }

    for mcp_config in &config.mcp_config {
        command.arg("--mcp-config").arg(mcp_config);
    }

    if background {
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let mut child =
            command.spawn().with_context(|| format!("Failed to spawn claude in {worktree:?}"))?;
        let pid = Some(child.id());
        let status =
            child.wait().with_context(|| format!("Failed to wait for claude in {worktree:?}"))?;
        Ok(RuntimeOutcome { status, pid })
    } else if config.interactive {
        // Interactive mode: inherit all stdio
        command.stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit());

        let mut child =
            command.spawn().with_context(|| format!("Failed to spawn claude in {worktree:?}"))?;
        let pid = Some(child.id());
        let status =
            child.wait().with_context(|| format!("Failed to wait for claude in {worktree:?}"))?;

        Ok(RuntimeOutcome { status, pid })
    } else {
        // Process stdout to convert JSON stream to readable text
        command.stdin(Stdio::inherit()).stdout(Stdio::piped()).stderr(Stdio::inherit());

        let mut child =
            command.spawn().with_context(|| format!("Failed to spawn claude in {worktree:?}"))?;
        let pid = Some(child.id());

        // Process the stdout stream
        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);
            if let Err(e) = process_claude_stream(reader) {
                eprintln!("Warning: error processing claude stream: {e}");
            }
        }

        let status =
            child.wait().with_context(|| format!("Failed to wait for claude in {worktree:?}"))?;

        Ok(RuntimeOutcome { status, pid })
    }
}
