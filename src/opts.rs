use clap::{ColorChoice, Parser};

#[derive(Parser)]
#[clap(version = "0.1.0")]
#[clap(color = ColorChoice::Auto)]
pub struct CmdLineOpts {
    /// File to analyze.
    pub target: String,
}
