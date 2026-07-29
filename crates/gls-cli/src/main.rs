use gls_science::{
    ScienceCommand, ScienceOptions, StoreOptions, run_science_command, run_store_command,
};
use std::path::{Path, PathBuf};

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match run_cli(&args) {
        Ok(output) => println!("{output}"),
        Err(reason) => {
            eprintln!("{reason}");
            std::process::exit(if reason.starts_with("pipeline_blocked") {
                2
            } else {
                1
            });
        }
    }
}

fn run_cli(args: &[String]) -> Result<String, String> {
    if args.is_empty() || is_help(args) {
        return Ok(HELP.to_string());
    }
    if args.len() == 1 && matches!(args[0].as_str(), "--version" | "-V") {
        return Ok(format!(
            "gls-cli {} commit {} build_id {}",
            env!("CARGO_PKG_VERSION"),
            env!("GENOLIGAND_BUILD_COMMIT"),
            env!("GENOLIGAND_BUILD_ID")
        ));
    }
    let group = args[0].as_str();
    if args.len() < 2 {
        return Err(format!("missing_subcommand:{group}"));
    }
    let command = ScienceCommand::parse(group, &args[1])?;
    let common = CommonArgs::parse(command, &args[2..])?;
    if command == ScienceCommand::PrepareScienceStore {
        return run_store_command(
            command,
            &StoreOptions {
                workspace_root: common.workspace_root,
                science_store: common.science_store,
            },
        );
    }
    run_science_command(
        command,
        &ScienceOptions {
            workspace_root: common.workspace_root,
            science_store: common.science_store,
            input_manifest: common.input_manifest,
            run_dir: common.run_dir,
        },
    )
}

#[derive(Debug)]
struct CommonArgs {
    workspace_root: PathBuf,
    science_store: PathBuf,
    input_manifest: PathBuf,
    run_dir: PathBuf,
}

impl CommonArgs {
    fn parse(command: ScienceCommand, args: &[String]) -> Result<Self, String> {
        let mut workspace_root = None;
        let mut science_store = None;
        let mut input_manifest = None;
        let mut run_dir = None;
        let mut index = 0usize;
        while index < args.len() {
            match args[index].as_str() {
                "--workspace-root" => {
                    workspace_root = Some(take_path(args, &mut index, "--workspace-root")?)
                }
                "--science-store" => {
                    science_store = Some(take_path(args, &mut index, "--science-store")?)
                }
                "--input-manifest" => {
                    input_manifest = Some(take_path(args, &mut index, "--input-manifest")?)
                }
                "--run-dir" | "--out-dir" => {
                    run_dir = Some(take_path(args, &mut index, "--run-dir")?)
                }
                other => return Err(format!("unknown_option:{other}")),
            }
            index += 1;
        }
        let (workspace_root, science_store, input_manifest, run_dir) =
            if command == ScienceCommand::PrepareScienceStore {
                let workspace_root = workspace_root
                    .unwrap_or(std::env::current_dir().map_err(|err| err.to_string())?);
                let science_store = science_store
                    .or_else(|| {
                        std::env::var("GENOLIGAND_SCIENCE_STORE")
                            .ok()
                            .filter(|value| !value.trim().is_empty())
                            .map(PathBuf::from)
                    })
                    .unwrap_or_else(|| default_science_store(&workspace_root));
                (
                    workspace_root,
                    science_store,
                    PathBuf::new(),
                    PathBuf::new(),
                )
            } else {
                (
                    workspace_root.ok_or("missing_required_option:--workspace-root")?,
                    science_store.ok_or("missing_required_option:--science-store")?,
                    input_manifest.ok_or("missing_required_option:--input-manifest")?,
                    run_dir.ok_or("missing_required_option:--run-dir")?,
                )
            };
        Ok(Self {
            workspace_root,
            science_store,
            input_manifest,
            run_dir,
        })
    }
}

fn default_science_store(workspace_root: &Path) -> PathBuf {
    if let Ok(root) = std::env::var("GENOLIGAND_DATA_ROOT") {
        let root = PathBuf::from(root);
        if !root.as_os_str().is_empty() {
            return root.join("science_store");
        }
    }
    if cfg!(windows) {
        let d_root = PathBuf::from(r"D:\GenoLigandData\science_store");
        if Path::new(r"D:\").exists() {
            return d_root;
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(local)
                .join("GenoLigand")
                .join("science_store");
        }
    } else {
        let data_root = PathBuf::from("/data/genoligand/science_store");
        if Path::new("/data").exists() {
            return data_root;
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("genoligand")
                .join("science_store");
        }
    }
    workspace_root.join(".genoligand").join("science_store")
}

fn take_path(args: &[String], index: &mut usize, flag: &str) -> Result<PathBuf, String> {
    *index += 1;
    let value = args
        .get(*index)
        .ok_or_else(|| format!("missing_value_for:{flag}"))?;
    if value.trim().is_empty() {
        return Err(format!("empty_value_for:{flag}"));
    }
    Ok(PathBuf::from(value))
}

fn is_help(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--help" || arg == "-h")
}

const HELP: &str = "\
usage:
  gls-cli install prepare-science-store [--science-store PATH] [--workspace-root PATH]
  gls-cli input normalize --workspace-root PATH --science-store PATH --input-manifest PATH --run-dir PATH
  gls-cli features compute --workspace-root PATH --science-store PATH --input-manifest PATH --run-dir PATH
  gls-cli database match-known --workspace-root PATH --science-store PATH --input-manifest PATH --run-dir PATH
  gls-cli metagenome match-potential --workspace-root PATH --science-store PATH --input-manifest PATH --run-dir PATH
  gls-cli structure evaluate --workspace-root PATH --science-store PATH --input-manifest PATH --run-dir PATH
  gls-cli candidate coarse-rank --workspace-root PATH --science-store PATH --input-manifest PATH --run-dir PATH
  gls-cli dynamics screen --workspace-root PATH --science-store PATH --input-manifest PATH --run-dir PATH
  gls-cli target rank --workspace-root PATH --science-store PATH --input-manifest PATH --run-dir PATH
  gls-cli evidence build --workspace-root PATH --science-store PATH --input-manifest PATH --run-dir PATH
  gls-cli report render --workspace-root PATH --science-store PATH --input-manifest PATH --run-dir PATH
  gls-cli pipeline finalize --workspace-root PATH --science-store PATH --input-manifest PATH --run-dir PATH
  gls-cli pipeline run --workspace-root PATH --science-store PATH --input-manifest PATH --run-dir PATH
";

#[cfg(test)]
mod tests {
    use super::*;

    const SCIENCE_COMMANDS: [ScienceCommand; 12] = [
        ScienceCommand::InputNormalize,
        ScienceCommand::FeaturesCompute,
        ScienceCommand::DatabaseMatchKnown,
        ScienceCommand::MetagenomeMatchPotential,
        ScienceCommand::StructureEvaluate,
        ScienceCommand::CandidateCoarseRank,
        ScienceCommand::DynamicsScreen,
        ScienceCommand::TargetRank,
        ScienceCommand::EvidenceBuild,
        ScienceCommand::ReportRender,
        ScienceCommand::PipelineFinalize,
        ScienceCommand::PipelineRun,
    ];

    const REQUIRED_PATHS: [(&str, &str); 4] = [
        ("--workspace-root", "workspace"),
        ("--science-store", "store"),
        ("--input-manifest", "input.json"),
        ("--run-dir", "run"),
    ];

    fn science_args() -> Vec<String> {
        REQUIRED_PATHS
            .iter()
            .flat_map(|(flag, value)| [flag.to_string(), value.to_string()])
            .collect()
    }

    #[test]
    fn science_commands_require_all_documented_paths() {
        for command in SCIENCE_COMMANDS {
            for (flag, _) in REQUIRED_PATHS {
                let mut args = science_args();
                let position = args.iter().position(|arg| arg == flag).expect("test flag");
                args.drain(position..=position + 1);

                let error = CommonArgs::parse(command, &args)
                    .expect_err("science command path must be required");
                assert_eq!(error, format!("missing_required_option:{flag}"));
            }
        }
    }

    #[test]
    fn science_commands_reject_blank_path_values() {
        for (flag, _) in REQUIRED_PATHS {
            let mut args = science_args();
            let position = args.iter().position(|arg| arg == flag).expect("test flag");
            args[position + 1] = " \t".to_string();

            let error = CommonArgs::parse(ScienceCommand::InputNormalize, &args)
                .expect_err("blank science command path must fail");
            assert_eq!(error, format!("empty_value_for:{flag}"));
        }
    }

    #[test]
    fn install_command_keeps_workspace_and_store_defaults() {
        let parsed =
            CommonArgs::parse(ScienceCommand::PrepareScienceStore, &[]).expect("install defaults");

        assert_eq!(parsed.workspace_root, std::env::current_dir().unwrap());
        assert!(!parsed.science_store.as_os_str().is_empty());
        assert!(parsed.input_manifest.as_os_str().is_empty());
        assert!(parsed.run_dir.as_os_str().is_empty());
    }

    #[test]
    fn version_matches_workspace_package_version() {
        let output = run_cli(&["--version".to_string()]).expect("version output");
        assert_eq!(
            output,
            format!(
                "gls-cli {} commit {} build_id {}",
                env!("CARGO_PKG_VERSION"),
                env!("GENOLIGAND_BUILD_COMMIT"),
                env!("GENOLIGAND_BUILD_ID")
            )
        );
    }

    #[test]
    fn help_marks_only_install_paths_as_optional() {
        let usage = HELP
            .lines()
            .filter(|line| line.starts_with("  gls-cli "))
            .collect::<Vec<_>>();
        let (install, science) = usage.split_first().expect("usage lines");

        assert!(install.contains("[--science-store PATH]"));
        assert!(install.contains("[--workspace-root PATH]"));
        for line in science {
            for (flag, _) in REQUIRED_PATHS {
                assert!(line.contains(&format!("{flag} PATH")), "{line}");
            }
            assert!(!line.contains('['), "{line}");
        }
    }
}
