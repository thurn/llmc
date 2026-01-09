mod accept;
mod clean;
mod cli;
mod config;
mod r#continue;
mod delete;
mod git_ops;
mod notify;
mod nouns;
mod prompt;
mod rebase;
mod reject;
mod review;
mod runtime;
mod setup;
mod start;
mod state;
mod status;
mod time;

use std::process;

use anyhow::Result;
use clap::Parser;

use crate::cli::{Cli, Commands};

fn main() {
    if let Err(err) = self::run() {
        eprintln!("{err:#}");
        process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Setup(args) => {
            setup::run(&args, cli.repo.as_deref())?;
        }
        Commands::Start(args) => {
            start::run(&args, cli.repo.as_deref())?;
        }
        Commands::Status(args) => {
            status::run(&args, cli.repo.as_deref())?;
        }
        Commands::Rebase(args) => {
            rebase::run(&args, cli.repo.as_deref())?;
        }
        Commands::Review(args) => {
            review::run(&args, cli.repo.as_deref())?;
        }
        Commands::Delete(args) => {
            delete::run(&args, cli.repo.as_deref())?;
        }
        Commands::Clean(args) => {
            clean::run(&args, cli.repo.as_deref())?;
        }
        Commands::Reject(args) => {
            reject::run(&args, cli.repo.as_deref())?;
        }
        Commands::Accept(args) => {
            accept::run(&args, cli.repo.as_deref())?;
        }
        Commands::Continue(args) => {
            r#continue::run(&args, cli.repo.as_deref())?;
        }
    }

    Ok(())
}
