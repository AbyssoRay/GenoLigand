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
            std::process::exit(1);
        }
    }
}

fn run_cli(args: &[String]) -> Result<String, String> {
    if args.is_empty() || is_help(args) {
        return Ok(HELP.to_string());
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
        let workspace_root =
            workspace_root.unwrap_or(std::env::current_dir().map_err(|err| err.to_string())?);
        let science_store = science_store
            .or_else(|| {
                std::env::var("GENOLIGAND_SCIENCE_STORE")
                    .ok()
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| default_science_store(&workspace_root));
        let (input_manifest, run_dir) = if command == ScienceCommand::PrepareScienceStore {
            (PathBuf::new(), PathBuf::new())
        } else {
            (
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
  gls-cli input normalize --input-manifest PATH --run-dir PATH [--science-store PATH]
  gls-cli features compute --input-manifest PATH --run-dir PATH [--science-store PATH]
  gls-cli database match-known --input-manifest PATH --run-dir PATH [--science-store PATH]
  gls-cli metagenome match-potential --input-manifest PATH --run-dir PATH [--science-store PATH]
  gls-cli structure evaluate --input-manifest PATH --run-dir PATH [--science-store PATH]
  gls-cli dynamics screen --input-manifest PATH --run-dir PATH [--science-store PATH]
  gls-cli target rank --input-manifest PATH --run-dir PATH [--science-store PATH]
  gls-cli evidence build --input-manifest PATH --run-dir PATH [--science-store PATH]
  gls-cli report render --input-manifest PATH --run-dir PATH [--science-store PATH]
  gls-cli pipeline run --input-manifest PATH --run-dir PATH [--science-store PATH]
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn science_command_requires_input_manifest_and_run_dir() {
        let args = vec!["--science-store".into(), "store".into()];
        let error = CommonArgs::parse(ScienceCommand::InputNormalize, &args)
            .expect_err("science paths must be required");
        assert_eq!(error, "missing_required_option:--input-manifest");
    }

    #[test]
    fn path_options_reject_empty_values() {
        let args = vec!["--science-store".into(), "".into()];
        let error = CommonArgs::parse(ScienceCommand::PrepareScienceStore, &args)
            .expect_err("empty path must fail");
        assert_eq!(error, "empty_value_for:--science-store");
    }

    #[test]
    fn install_command_does_not_require_run_paths() {
        let args = vec!["--science-store".into(), "store".into()];
        let parsed =
            CommonArgs::parse(ScienceCommand::PrepareScienceStore, &args).expect("install args");
        assert_eq!(parsed.science_store, PathBuf::from("store"));
    }
}
