use gls_cli::{
    DevSmokeInputs, WorkerCommandSet, run_dev_smoke_with_worker_commands_and_inputs,
    run_worker_command,
};
use gls_report::{render_html, render_markdown};
use std::path::PathBuf;

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let command = args
        .first()
        .cloned()
        .unwrap_or_else(|| "dev-smoke".to_string());
    if command.starts_with("worker-") {
        if let Err(reason) = run_worker_command(&command) {
            eprintln!("{reason}");
            std::process::exit(1);
        }
        return;
    }
    let worker_bin =
        std::env::current_exe().map_err(|err| format!("current_exe_unavailable:{err}"));
    let result = match command.as_str() {
        "dev-smoke" => worker_bin
            .and_then(|path| run_dev_smoke_cli(&path, &args[1..]))
            .map(|report| render_markdown(&report)),
        "dev-smoke-html" => worker_bin
            .and_then(|path| run_dev_smoke_cli(&path, &args[1..]))
            .map(|report| render_html(&report)),
        _ => Err(format!("unknown_command:{command}")),
    };
    match result {
        Ok(output) => println!("{output}"),
        Err(reason) => {
            eprintln!("{reason}");
            std::process::exit(1);
        }
    }
}

fn run_dev_smoke_cli(
    worker_bin: &std::path::Path,
    args: &[String],
) -> Result<gls_types::EndToEndReport, String> {
    let options = DevSmokeOptions::parse(args)?;
    let inputs = DevSmokeInputs::from_optional_files(
        options.ligand_yaml.as_deref(),
        options.genome_fasta.as_deref(),
    )?;
    run_dev_smoke_with_worker_commands_and_inputs(
        WorkerCommandSet::from_env_or_dev(worker_bin),
        inputs,
    )
}

#[derive(Default)]
struct DevSmokeOptions {
    ligand_yaml: Option<PathBuf>,
    genome_fasta: Option<PathBuf>,
}

impl DevSmokeOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Self::default();
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--ligand-yaml" => {
                    index += 1;
                    let value = args
                        .get(index)
                        .ok_or_else(|| "missing_value_for:--ligand-yaml".to_string())?;
                    options.ligand_yaml = Some(PathBuf::from(value));
                }
                "--genome-fasta" => {
                    index += 1;
                    let value = args
                        .get(index)
                        .ok_or_else(|| "missing_value_for:--genome-fasta".to_string())?;
                    options.genome_fasta = Some(PathBuf::from(value));
                }
                "--help" | "-h" => {
                    return Err(
                        "usage: gls-cli dev-smoke [--ligand-yaml PATH] [--genome-fasta PATH]"
                            .to_string(),
                    );
                }
                other => return Err(format!("unknown_dev_smoke_option:{other}")),
            }
            index += 1;
        }
        Ok(options)
    }
}
