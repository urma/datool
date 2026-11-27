mod archive;
mod cli;
mod db;
mod dedupe;
mod hash;
mod matcher;
mod scanner;

use clap::Parser;

fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();
    cli.run()
}
