# LLMC

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

A Rust CLI that coordinates multiple AI agents working in parallel git worktrees.

## Overview

LLMC creates isolated git worktrees for each AI agent, runs agents headlessly, manages conflict resolution using git rerere, and handles clean rebasing and fast-forward merging back to the main branch.

```
┌─────────────────────────────────────────────────────────────┐
│                     Main Repository                         │
│  master ─────────────────────────────────────────────────► │
│                                                             │
│  .worktrees/                                                │
│  ├── agent-pineapple/  ──► claude (feature A)              │
│  ├── agent-dalmatian/  ──► codex (feature B)               │
│  └── agent-telescope/  ──► gemini (bugfix C)               │
│                                                             │
│  .llmc/                                                     │
│  ├── state.json        (agent registry)                     │
│  └── logs/             (optional run logs)                  │
└─────────────────────────────────────────────────────────────┘
```

## Installation

### From source

```bash
git clone https://github.com/dthurn/llmc
cd llmc
cargo install --path .
```

### From crates.io

```bash
cargo install llmc
```

## Quick Start

1. **Set up a new LLMC-managed checkout:**

```bash
llmc setup --source ~/my-project --target ~/llmc-workspace
cd ~/llmc-workspace
```

2. **Start an agent:**

```bash
llmc start --runtime claude --prompt "Add input validation to the user form"
```

3. **Review the agent's work:**

```bash
llmc review --interface difftastic
```

4. **Accept or reject:**

```bash
# Accept and merge to master
llmc accept

# Or reject with feedback
llmc reject --notes "Please also add unit tests"
```

## CLI Reference

### Global Options

| Option | Description |
|--------|-------------|
| `--repo <PATH>` | Override automatic repo root detection |

### llmc setup

Initialize a new repository checkout for LLMC coordination.

```bash
llmc setup --source <PATH> --target <PATH>
```

| Option | Description |
|--------|-------------|
| `--source <PATH>` | Source checkout to clone from |
| `--target <PATH>` | Target path for the new checkout |

This command:
- Clones the repository using local clone for efficiency
- Enables git rerere for conflict resolution
- Installs and pulls Git LFS
- Creates `.llmc/` and `.worktrees/` directories
- Validates that `.llmc/` is in `.gitignore`

### llmc start

Create a new agent worktree and run an AI agent.

```bash
llmc start --runtime <RUNTIME> --prompt "Your task description"
```

| Option | Description |
|--------|-------------|
| `--agent <ID>` | Agent identifier (auto-generated if omitted) |
| `--runtime <RUNTIME>` | Runtime to use: `claude`, `codex`, `gemini`, `cursor` |
| `--prompt <TEXT>` | Prompt text for the agent |
| `--prompt-file <PATH>` | File containing prompt text (can be repeated) |
| `--prompt-pool <PATH>` | Pool file to select next unimplemented prompt |
| `--background` | Run without streaming output |
| `--no-notify` | Disable completion notification |
| `--log` | Write logs to `.llmc/logs/` |

**Claude-specific options:**

| Option | Description |
|--------|-------------|
| `--claude-model <MODEL>` | Model to use (e.g., `sonnet`, `opus`) |
| `--claude-no-thinking` | Disable thinking mode |
| `--claude-sandbox <MODE>` | Sandbox mode |
| `--claude-skip-permissions` | Skip permission prompts |
| `--claude-allowed-tools <TOOLS>` | Comma-separated list of allowed tools |
| `--claude-mcp-config <CONFIGS>` | MCP config files or JSON strings |
| `--claude-interactive` | Run in interactive mode |

### llmc status

Display the status of an agent.

```bash
llmc status --agent <ID>
```

### llmc rebase

Rebase an agent branch onto the latest master.

```bash
llmc rebase --agent <ID>
```

If conflicts occur, the agent is re-invoked with a conflict-resolution prompt.

### llmc review

Present diffs for review.

```bash
llmc review --agent <ID> --interface <INTERFACE>
```

| Option | Description |
|--------|-------------|
| `--agent <ID>` | Agent to review (uses oldest pending if omitted) |
| `--interface <INTERFACE>` | Review interface: `diff`, `difftastic`, `vscode`, `forgejo` |
| `--force` | Allow reviewing running agents |

### llmc reject

Send feedback to an agent for fixes.

```bash
llmc reject --agent <ID> --notes "Your feedback"
```

| Option | Description |
|--------|-------------|
| `--agent <ID>` | Agent to reject (uses last reviewed if omitted) |
| `--notes <TEXT>` | Reviewer notes |
| `--notes-file <PATH>` | File containing notes |
| `--include-prompt` | Include original prompt context |

### llmc accept

Accept agent work and merge to master.

```bash
llmc accept --agent <ID>
```

| Option | Description |
|--------|-------------|
| `--agent <ID>` | Agent to accept (uses last reviewed if omitted) |
| `--nopull` | Skip rebasing onto latest master |

This command:
- Rebases the agent branch onto master
- Squashes commits and cleans the commit message
- Fast-forward merges to master
- Removes the worktree and branch
- Cleans up the agent state

### llmc continue

Continue an agent's work with optional new instructions.

```bash
llmc continue --agent <ID> --notes "Additional instructions"
```

| Option | Description |
|--------|-------------|
| `--agent <ID>` | Agent to continue |
| `--notes <TEXT>` | Additional instructions |
| `--notes-file <PATH>` | File containing notes |
| `--runtime <RUNTIME>` | Override stored runtime |

Also accepts Claude-specific options (same as `llmc start`).

### llmc delete

Remove a single agent and its worktree.

```bash
llmc delete --agent <ID>
```

### llmc clean

Remove all agents and reset LLMC state.

```bash
llmc clean
```

## Supported Runtimes

| Runtime | Command | Status |
|---------|---------|--------|
| Claude | `claude -p <PROMPT> --output-format stream-json` | Supported |
| Codex | `codex exec --ask-for-approval never <PROMPT>` | Supported |
| Gemini | `gemini -p <PROMPT>` | Planned |
| Cursor | `cursor --print <PROMPT>` | Planned |

## State Management

LLMC stores all agent state in `.llmc/state.json`:

```json
{
  "agents": {
    "pineapple": {
      "agent_id": "pineapple",
      "branch": "agent/pineapple",
      "worktree_path": "/path/to/.worktrees/agent-pineapple",
      "runtime": "claude",
      "prompt": "...",
      "status": "needs_review",
      "created_at_unix": 1704825600,
      "last_run_unix": 1704825700
    }
  }
}
```

Agent statuses:
- `idle` - Agent created but not running
- `running` - Agent currently executing
- `rebasing` - Handling rebase conflicts
- `needs_review` - Work complete, awaiting review
- `accepted` - Work merged to master
- `rejected` - Sent back for fixes

## Requirements

- Rust 1.70+
- Git with LFS support
- At least one supported AI agent CLI installed:
  - [Claude Code](https://claude.ai/code) (`claude`)
  - [OpenAI Codex](https://openai.com/codex) (`codex`)

Optional for review interfaces:
- [difftastic](https://difftastic.wilfred.me.uk/) (`difft`) for syntax-aware diffs

## Directory Structure

```
your-repo/
├── .llmc/
│   ├── state.json     # Agent registry
│   └── logs/          # Optional run logs
├── .worktrees/
│   └── agent-<id>/    # Per-agent worktrees
└── ...
```

## License

Apache 2.0 - see [LICENSE](LICENSE) for details.
